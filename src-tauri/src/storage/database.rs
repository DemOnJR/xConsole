use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

use super::models::{
    AgentApproval, AgentConversation, AgentConversationInput, AgentConversationMeta,
    AiProvider, AiProviderInput, AuthType, CloudAccount, CloudAccountInput,
    CronJob, CronJobInput, InfraProject, InfraProjectInput, KnownHost, Vps, VpsInput, Workspace,
    WorkspaceInput,
};
use crate::ai::conversations;
use crate::ai::provider::ChatMessage;
use crate::secrets;

/// Result of a trust-on-first-use host key check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerdict {
    /// First time we've seen this host; the fingerprint was just pinned.
    PinnedOnFirstUse,
    /// Fingerprint matches the previously pinned key.
    Match,
    /// Fingerprint does NOT match the pinned key (possible MITM) - connection rejected.
    Mismatch { expected: String },
}

/// Thread-safe handle to the local SQLite database.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// Present when the DB is encrypted at rest (Approach B); drives the persister.
    /// Interior-mutable so a locked placeholder can be unlocked in place (the connection
    /// is swapped and this is set) without re-creating the managed Db — see `unlock_into`.
    persist: Arc<Mutex<Option<Arc<super::encrypt::PersistCtx>>>>,
}

impl Db {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
            persist: Arc::new(Mutex::new(None)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// A locked placeholder: an empty in-memory DB so managed state can be built at startup
    /// before the user has unlocked. The frontend shows only the unlock screen; `unlock_into`
    /// swaps the real decrypted connection in once the password/key is available.
    pub fn open_locked() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
            persist: Arc::new(Mutex::new(None)),
        };
        db.migrate()?; // harmless empty schema; replaced on unlock
        Ok(db)
    }

    /// Open the DB with at-rest encryption. `enc` is the encrypted blob, `work` the plaintext
    /// working file the app operates on, `key` the 32-byte data key (from unlock). Decrypts
    /// `enc` → `work` (unless a valid plaintext `work` already exists from an unclean shutdown —
    /// that is always at least as new as `enc`, so it's preferred for crash recovery), then
    /// opens `work` exactly like [`open`]. A wrong/corrupt key returns Err (caller shows a
    /// locked/restore screen) rather than panicking.
    pub fn open_encrypted(
        enc: &std::path::Path,
        work: &std::path::Path,
        data_dir: &std::path::Path,
        key: &[u8; crate::crypto::KEY_LEN],
    ) -> Result<Self> {
        use super::encrypt;
        if let Some(parent) = work.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // A half-written blob from a kill mid-persist — discard it.
        let _ = std::fs::remove_file(enc.with_extension("enc.tmp"));

        // Lazy plaintext cleanup: a clean exit leaves a `.clean` marker but can't delete the
        // still-open working file on Windows. So at the NEXT launch (file now closeable) we
        // delete that stale plaintext and decrypt fresh from `.enc`. No marker => the previous
        // run crashed, so a valid working file is the most-recent truth and is kept.
        let clean_marker = enc.with_extension("clean");
        let had_clean_exit = clean_marker.exists();
        let _ = std::fs::remove_file(&clean_marker);
        if had_clean_exit && work.exists() {
            encrypt::cleanup_work_files(work);
        }

        let have_valid_work = work.exists() && encrypt::integrity_ok(work);
        if !have_valid_work {
            if work.exists() {
                encrypt::cleanup_work_files(work); // corrupt leftover
            }
            if enc.exists() {
                encrypt::decrypt_to_work(enc, work, key)?; // wrong key => Err here
            }
            // else: first run — Connection::open will create an empty `work`.
        }

        let conn = Connection::open(work)?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();

        let dirty = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Flag dirty on every committed write (autocommit => once per execute).
        conn.commit_hook(Some({
            let d = dirty.clone();
            move || {
                d.store(true, std::sync::atomic::Ordering::Release);
                false // allow the commit
            }
        }));

        let conn = Arc::new(Mutex::new(conn));
        let ctx = Arc::new(encrypt::PersistCtx {
            enc: enc.to_path_buf(),
            work: work.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            key: *key,
            dirty,
            stopped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        let db = Db {
            conn: conn.clone(),
            persist: Arc::new(Mutex::new(Some(ctx.clone()))),
        };
        db.migrate()?;
        // First run with no blob yet: write the initial encrypted artifact now.
        if !enc.exists() {
            encrypt::persist_now(&conn, &ctx)?;
        }
        encrypt::spawn_persister(conn, ctx);
        Ok(db)
    }

    /// Unlock a locked placeholder IN PLACE: decrypt `enc`→`work`, swap the real connection
    /// into this Db (every clone shares the same Arc, so they all pick it up), wire the
    /// commit-hook + persister, and migrate. Used by the unlock command at runtime.
    pub fn unlock_into(
        &self,
        enc: &std::path::Path,
        work: &std::path::Path,
        data_dir: &std::path::Path,
        key: &[u8; crate::crypto::KEY_LEN],
    ) -> Result<()> {
        use super::encrypt;
        if let Some(parent) = work.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let _ = std::fs::remove_file(enc.with_extension("enc.tmp"));
        let clean_marker = enc.with_extension("clean");
        let had_clean_exit = clean_marker.exists();
        let _ = std::fs::remove_file(&clean_marker);
        if had_clean_exit && work.exists() {
            encrypt::cleanup_work_files(work);
        }
        let have_valid_work = work.exists() && encrypt::integrity_ok(work);
        if !have_valid_work {
            if work.exists() {
                encrypt::cleanup_work_files(work);
            }
            if enc.exists() {
                encrypt::decrypt_to_work(enc, work, key)?; // wrong key => Err
            }
        }

        let new_conn = Connection::open(work)?;
        new_conn.pragma_update(None, "journal_mode", "WAL").ok();
        new_conn.pragma_update(None, "foreign_keys", "ON").ok();
        let dirty = Arc::new(std::sync::atomic::AtomicBool::new(false));
        new_conn.commit_hook(Some({
            let d = dirty.clone();
            move || {
                d.store(true, std::sync::atomic::Ordering::Release);
                false
            }
        }));

        // Swap the real connection in (replaces the in-memory placeholder).
        *self.conn.lock().unwrap() = new_conn;

        let ctx = Arc::new(encrypt::PersistCtx {
            enc: enc.to_path_buf(),
            work: work.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            key: *key,
            dirty,
            stopped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        self.migrate()?; // run on the now-real connection
        if !enc.exists() {
            encrypt::persist_now(&self.conn, &ctx)?;
        }
        *self.persist.lock().unwrap() = Some(ctx.clone());
        encrypt::spawn_persister(self.conn.clone(), ctx);
        Ok(())
    }

    /// Synchronously snapshot + encrypt the DB now (no-op for an unencrypted DB). Call after a
    /// security-critical write (host-key pin) and on clean app exit so a crash can't drop it.
    /// Consistent Online-Backup snapshot of the live DB to `dst` (a fresh SQLite file). Safe
    /// against concurrent writers; used for the pre-migration backup + the encrypt snapshot.
    pub fn backup_to(&self, dst: &std::path::Path) -> Result<()> {
        let _ = std::fs::remove_file(dst);
        let src = self.conn.lock().unwrap();
        let _ = src.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        let mut out = Connection::open(dst)?;
        {
            let backup = rusqlite::backup::Backup::new(&src, &mut out)?;
            backup.run_to_completion(200, std::time::Duration::from_millis(0), None)?;
        }
        Ok(())
    }

    /// Convert the currently-open (plaintext) DB into an encrypted one IN PLACE: attach the
    /// commit-hook + persister so future writes persist to `enc`. The caller has already
    /// written + verified the initial `enc`, and the current working file IS `work`.
    pub fn enable_encryption_in_place(
        &self,
        enc: &std::path::Path,
        work: &std::path::Path,
        data_dir: &std::path::Path,
        key: &[u8; crate::crypto::KEY_LEN],
    ) -> Result<()> {
        use super::encrypt;
        let dirty = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let conn = self.conn.lock().unwrap();
            conn.commit_hook(Some({
                let d = dirty.clone();
                move || {
                    d.store(true, std::sync::atomic::Ordering::Release);
                    false
                }
            }));
        }
        let ctx = Arc::new(encrypt::PersistCtx {
            enc: enc.to_path_buf(),
            work: work.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            key: *key,
            dirty,
            stopped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        *self.persist.lock().unwrap() = Some(ctx.clone());
        encrypt::spawn_persister(self.conn.clone(), ctx);
        Ok(())
    }

    /// Disable encryption: decrypt to plaintext working file (already there), drop the persister
    /// ctx + commit-hook, and the caller removes `enc`. Used by "turn off app lock".
    pub fn disable_encryption(&self) {
        if let Some(ctx) = self.persist.lock().unwrap().take() {
            ctx.stopped.store(true, std::sync::atomic::Ordering::Release);
        }
        let conn = self.conn.lock().unwrap();
        conn.commit_hook::<fn() -> bool>(None);
    }

    pub fn persist_now_blocking(&self) -> Result<()> {
        let ctx = self.persist.lock().unwrap().clone();
        if let Some(ctx) = ctx {
            super::encrypt::persist_now(&self.conn, &ctx)?;
        }
        Ok(())
    }

    /// Whether this DB is encrypted at rest (i.e. unlocked with a key, not a plain/placeholder DB).
    pub fn is_encrypted(&self) -> bool {
        self.persist.lock().unwrap().is_some()
    }

    /// On clean exit: final persist + drop a `.clean` marker. We can't delete the still-open
    /// plaintext working file on Windows, so the next launch removes it (see `open_encrypted`).
    /// No-op for an unencrypted DB.
    pub fn finalize_on_exit(&self) {
        let ctx = self.persist.lock().unwrap().clone();
        if let Some(ctx) = ctx {
            let _ = super::encrypt::persist_now(&self.conn, &ctx);
            let _ = std::fs::write(ctx.enc.with_extension("clean"), b"1");
        }
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS vps (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                host        TEXT NOT NULL,
                port        INTEGER NOT NULL DEFAULT 22,
                username    TEXT NOT NULL,
                auth_type   TEXT NOT NULL DEFAULT 'key',
                key_path    TEXT,
                tags        TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS workspace (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                viewport_json TEXT,
                layout_mode   TEXT,
                nodes_json    TEXT,
                color         TEXT,
                icon          TEXT,
                color_mode    TEXT,
                updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS known_host (
                host        TEXT NOT NULL,
                port        INTEGER NOT NULL,
                key_type    TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                added_at    TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (host, port)
            );

            -- Generic key/value settings. Every settings category reads/writes
            -- through this one table (no per-category schema sprawl).
            CREATE TABLE IF NOT EXISTS setting (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Configured AI providers. Secrets (API keys / tokens) never live
            -- here; they go to the OS keychain under `ai:<id>:key`.
            CREATE TABLE IF NOT EXISTS ai_provider (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                kind        TEXT NOT NULL,
                model       TEXT,
                base_url    TEXT,
                bin_path    TEXT,
                extra_json  TEXT,
                enabled     INTEGER NOT NULL DEFAULT 1,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Scheduled agent jobs (prompt or raw command) against VPS targets.
            CREATE TABLE IF NOT EXISTS cron_job (
                id           TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                schedule     TEXT NOT NULL,
                kind         TEXT NOT NULL,
                payload      TEXT NOT NULL,
                targets_json TEXT,
                enabled      INTEGER NOT NULL DEFAULT 1,
                last_run     TEXT,
                last_status  TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Pending/!resolved approvals for agent commands (approve safety mode).
            CREATE TABLE IF NOT EXISTS agent_approval (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                vps_id      TEXT,
                command     TEXT NOT NULL,
                status      TEXT NOT NULL DEFAULT 'pending',
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Terraform / IaC projects (files live under agent home projects/).
            CREATE TABLE IF NOT EXISTS infra_project (
                id              TEXT PRIMARY KEY,
                name            TEXT NOT NULL,
                slug            TEXT NOT NULL UNIQUE,
                template        TEXT NOT NULL DEFAULT 'blank',
                backend         TEXT NOT NULL DEFAULT 'vps',
                default_vps_id  TEXT,
                cloud_account_id TEXT,
                config_json     TEXT,
                description     TEXT,
                created_at      TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS cloud_account (
                id           TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                kind         TEXT NOT NULL,
                region       TEXT,
                project_id   TEXT,
                organization TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS agent_conversation (
                id            TEXT PRIMARY KEY,
                title         TEXT NOT NULL,
                summary       TEXT,
                targets_json  TEXT,
                messages_json TEXT NOT NULL DEFAULT '[]',
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );
            "#,
        )?;

        // Add columns to pre-existing databases (ignore "duplicate column" errors).
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN color TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN icon TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN color_mode TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN project_json TEXT", []);
        let _ = conn.execute("ALTER TABLE vps ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN backend TEXT DEFAULT 'vps'", []);
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN cloud_account_id TEXT", []);
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN config_json TEXT", []);
        Ok(())
    }

    // ----- VPS CRUD -----

    pub fn list_vps(&self) -> Result<Vec<Vps>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, auth_type, key_path, tags, created_at
             FROM vps ORDER BY sort_order, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Vps {
                id: r.get(0)?,
                name: r.get(1)?,
                host: r.get(2)?,
                port: r.get::<_, i64>(3)? as u16,
                username: r.get(4)?,
                auth_type: AuthType::from_str(&r.get::<_, String>(5)?),
                key_path: r.get(6)?,
                tags: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_vps(&self, id: &str) -> Result<Option<Vps>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, auth_type, key_path, tags, created_at
             FROM vps WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(Vps {
                id: r.get(0)?,
                name: r.get(1)?,
                host: r.get(2)?,
                port: r.get::<_, i64>(3)? as u16,
                username: r.get(4)?,
                auth_type: AuthType::from_str(&r.get::<_, String>(5)?),
                key_path: r.get(6)?,
                tags: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    }

    pub fn upsert_vps(&self, input: &VpsInput) -> Result<Vps> {
        let id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO vps (id, name, host, port, username, auth_type, key_path, tags, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, COALESCE((SELECT MAX(sort_order) + 1 FROM vps), 0))
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    host = excluded.host,
                    port = excluded.port,
                    username = excluded.username,
                    auth_type = excluded.auth_type,
                    key_path = excluded.key_path,
                    tags = excluded.tags",
                params![
                    id,
                    input.name,
                    input.host,
                    input.port as i64,
                    input.username,
                    input.auth_type.as_str(),
                    input.key_path,
                    input.tags,
                ],
            )?;
        }
        Ok(self.get_vps(&id)?.expect("vps just upserted"))
    }

    pub fn delete_vps(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM vps WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Persist a manual ordering of the server list: each id's `sort_order`
    /// becomes its index in `ids`.
    pub fn reorder_vps(&self, ids: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE vps SET sort_order = ?1 WHERE id = ?2",
                params![i as i64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ----- Workspace CRUD -----

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, viewport_json, layout_mode, nodes_json, color, icon, color_mode, project_json, updated_at
             FROM workspace ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                viewport_json: r.get(2)?,
                layout_mode: r.get(3)?,
                nodes_json: r.get(4)?,
                color: r.get(5)?,
                icon: r.get(6)?,
                color_mode: r.get(7)?,
                project_json: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Fetch a single workspace by id.
    pub fn get_workspace(&self, id: &str) -> Result<Option<Workspace>> {
        Ok(self.list_workspaces()?.into_iter().find(|w| w.id == id))
    }

    pub fn upsert_workspace(&self, input: &WorkspaceInput) -> Result<Workspace> {
        let id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO workspace (id, name, viewport_json, layout_mode, nodes_json, color, icon, color_mode, project_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    viewport_json = excluded.viewport_json,
                    layout_mode = excluded.layout_mode,
                    nodes_json = excluded.nodes_json,
                    color = excluded.color,
                    icon = excluded.icon,
                    color_mode = excluded.color_mode,
                    project_json = excluded.project_json,
                    updated_at = datetime('now')",
                params![
                    id,
                    input.name,
                    input.viewport_json,
                    input.layout_mode,
                    input.nodes_json,
                    input.color,
                    input.icon,
                    input.color_mode,
                    input.project_json,
                ],
            )?;
        }
        let list = self.list_workspaces()?;
        Ok(list
            .into_iter()
            .find(|w| w.id == id)
            .expect("workspace just upserted"))
    }

    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM workspace WHERE id = ?1", [id])?;
        Ok(())
    }

    // ----- Generic settings (key/value) -----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        // Only "no row" maps to None; real DB errors propagate instead of being
        // silently swallowed (which previously masked e.g. a locked database).
        match conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM setting ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM setting WHERE key = ?1", [key])?;
        Ok(())
    }

    // ----- AI providers -----

    fn row_to_provider(r: &rusqlite::Row) -> rusqlite::Result<AiProvider> {
        Ok(AiProvider {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            model: r.get(3)?,
            base_url: r.get(4)?,
            bin_path: r.get(5)?,
            extra_json: r.get(6)?,
            enabled: r.get::<_, i64>(7)? != 0,
            has_secret: false,
            created_at: r.get(8)?,
        })
    }

    pub fn list_providers(&self) -> Result<Vec<AiProvider>> {
        let mut providers = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, model, base_url, bin_path, extra_json, enabled, created_at
                 FROM ai_provider ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], Self::row_to_provider)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for p in &mut providers {
            p.has_secret = secrets::has_secret(&secrets::provider_key(&p.id));
        }
        Ok(providers)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<AiProvider>> {
        let mut provider = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, model, base_url, bin_path, extra_json, enabled, created_at
                 FROM ai_provider WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map([id], Self::row_to_provider)?;
            match rows.next() {
                Some(v) => Some(v?),
                None => None,
            }
        };
        if let Some(p) = &mut provider {
            p.has_secret = secrets::has_secret(&secrets::provider_key(&p.id));
        }
        Ok(provider)
    }

    pub fn upsert_provider(&self, input: &AiProviderInput) -> Result<AiProvider> {
        let id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO ai_provider (id, name, kind, model, base_url, bin_path, extra_json, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    kind = excluded.kind,
                    model = excluded.model,
                    base_url = excluded.base_url,
                    bin_path = excluded.bin_path,
                    extra_json = excluded.extra_json,
                    enabled = excluded.enabled",
                params![
                    id,
                    input.name,
                    input.kind,
                    input.model,
                    input.base_url,
                    input.bin_path,
                    input.extra_json,
                    input.enabled as i64,
                ],
            )?;
        }
        Ok(self.get_provider(&id)?.expect("provider just upserted"))
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM ai_provider WHERE id = ?1", [id])?;
        Ok(())
    }

    // ----- Cron jobs -----

    fn row_to_cron(r: &rusqlite::Row) -> rusqlite::Result<CronJob> {
        Ok(CronJob {
            id: r.get(0)?,
            name: r.get(1)?,
            schedule: r.get(2)?,
            kind: r.get(3)?,
            payload: r.get(4)?,
            targets_json: r.get(5)?,
            enabled: r.get::<_, i64>(6)? != 0,
            last_run: r.get(7)?,
            last_status: r.get(8)?,
            created_at: r.get(9)?,
        })
    }

    pub fn list_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, kind, payload, targets_json, enabled, last_run, last_status, created_at
             FROM cron_job ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], Self::row_to_cron)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, kind, payload, targets_json, enabled, last_run, last_status, created_at
             FROM cron_job WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_cron(row)?)),
            None => Ok(None),
        }
    }

    pub fn upsert_cron_job(&self, input: &CronJobInput) -> Result<CronJob> {
        let id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO cron_job (id, name, schedule, kind, payload, targets_json, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    schedule = excluded.schedule,
                    kind = excluded.kind,
                    payload = excluded.payload,
                    targets_json = excluded.targets_json,
                    enabled = excluded.enabled",
                params![
                    id,
                    input.name,
                    input.schedule,
                    input.kind,
                    input.payload,
                    input.targets_json,
                    input.enabled as i64,
                ],
            )?;
        }
        let list = self.list_cron_jobs()?;
        Ok(list.into_iter().find(|c| c.id == id).expect("cron just upserted"))
    }

    pub fn delete_cron_job(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM cron_job WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn mark_cron_run(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE cron_job SET last_run = datetime('now'), last_status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    // ----- Agent approvals -----

    pub fn create_approval(
        &self,
        session_id: &str,
        vps_id: Option<&str>,
        command: &str,
    ) -> Result<AgentApproval> {
        let id = Uuid::new_v4().to_string();
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO agent_approval (id, session_id, vps_id, command, status)
                 VALUES (?1, ?2, ?3, ?4, 'pending')",
                params![id, session_id, vps_id, command],
            )?;
        }
        Ok(self.get_approval(&id)?.expect("approval just created"))
    }

    pub fn get_approval(&self, id: &str) -> Result<Option<AgentApproval>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, vps_id, command, status, created_at
             FROM agent_approval WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(AgentApproval {
                id: r.get(0)?,
                session_id: r.get(1)?,
                vps_id: r.get(2)?,
                command: r.get(3)?,
                status: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    }

    pub fn list_pending_approvals(&self) -> Result<Vec<AgentApproval>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, vps_id, command, status, created_at
             FROM agent_approval WHERE status = 'pending' ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AgentApproval {
                id: r.get(0)?,
                session_id: r.get(1)?,
                vps_id: r.get(2)?,
                command: r.get(3)?,
                status: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn resolve_approval(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE agent_approval SET status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    // ----- Known hosts (trust-on-first-use) -----

    pub fn list_known_hosts(&self) -> Result<Vec<KnownHost>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT host, port, key_type, fingerprint, added_at FROM known_host ORDER BY host",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(KnownHost {
                host: r.get(0)?,
                port: r.get::<_, i64>(1)? as u16,
                key_type: r.get(2)?,
                fingerprint: r.get(3)?,
                added_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Trust-on-first-use verification: pin the fingerprint on first sight, compare
    /// on subsequent connections, and reject on mismatch (possible MITM).
    pub fn verify_host_key(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
        fingerprint: &str,
    ) -> Result<HostKeyVerdict> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT fingerprint FROM known_host WHERE host = ?1 AND port = ?2",
                params![host, port as i64],
                |r| r.get(0),
            )
            .ok();
        match existing {
            Some(expected) if expected == fingerprint => Ok(HostKeyVerdict::Match),
            Some(expected) => Ok(HostKeyVerdict::Mismatch { expected }),
            None => {
                conn.execute(
                    "INSERT INTO known_host (host, port, key_type, fingerprint)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![host, port as i64, key_type, fingerprint],
                )?;
                Ok(HostKeyVerdict::PinnedOnFirstUse)
            }
        }
    }

    /// Forget a pinned host key (e.g. after a legitimate server key rotation).
    pub fn forget_host_key(&self, host: &str, port: u16) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM known_host WHERE host = ?1 AND port = ?2",
            params![host, port as i64],
        )?;
        Ok(())
    }

    // ----- Infra projects -----

    fn row_to_infra_project(r: &rusqlite::Row<'_>) -> rusqlite::Result<InfraProject> {
        Ok(InfraProject {
            id: r.get(0)?,
            name: r.get(1)?,
            slug: r.get(2)?,
            template: r.get(3)?,
            backend: r.get(4)?,
            default_vps_id: r.get(5)?,
            cloud_account_id: r.get(6)?,
            config_json: r.get(7)?,
            description: r.get(8)?,
            created_at: r.get(9)?,
        })
    }

    pub fn list_infra_projects(&self) -> Result<Vec<InfraProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, template, backend, default_vps_id, cloud_account_id,
                    config_json, description, created_at
             FROM infra_project ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], Self::row_to_infra_project)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_infra_project(&self, id: &str) -> Result<Option<InfraProject>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, template, backend, default_vps_id, cloud_account_id,
                    config_json, description, created_at
             FROM infra_project WHERE id = ?1 OR slug = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_infra_project(&row)?))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_infra_project(&self, input: &InfraProjectInput, slug: &str) -> Result<InfraProject> {
        // Resolve the id before insert: an explicit id wins; otherwise reuse the
        // row that already owns this slug so a re-save updates it (via
        // ON CONFLICT(id)) instead of tripping the slug UNIQUE constraint.
        let id = match input.id.clone().filter(|s| !s.is_empty()) {
            Some(id) => id,
            None => self
                .get_infra_project(slug)?
                .map(|p| p.id)
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        };
        let template = input
            .template
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "blank".to_string());
        let backend = input
            .backend
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "vps".to_string());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO infra_project (id, name, slug, template, backend, default_vps_id,
                cloud_account_id, config_json, description)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                slug = excluded.slug,
                template = excluded.template,
                backend = excluded.backend,
                default_vps_id = excluded.default_vps_id,
                cloud_account_id = excluded.cloud_account_id,
                config_json = excluded.config_json,
                description = excluded.description",
            params![
                id,
                input.name,
                slug,
                template,
                backend,
                input.default_vps_id,
                input.cloud_account_id,
                input.config_json,
                input.description,
            ],
        )?;
        self.get_infra_project(&id)?
            .ok_or_else(|| anyhow::anyhow!("project not found after upsert"))
    }

    pub fn delete_infra_project(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM infra_project WHERE id = ?1 OR slug = ?1", params![id])?;
        Ok(())
    }

    // ----- Cloud accounts -----

    fn row_to_cloud_account(r: &rusqlite::Row<'_>) -> rusqlite::Result<CloudAccount> {
        Ok(CloudAccount {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            region: r.get(3)?,
            project_id: r.get(4)?,
            organization: r.get(5)?,
            has_secret: false,
            created_at: r.get(6)?,
        })
    }

    pub fn list_cloud_accounts(&self) -> Result<Vec<CloudAccount>> {
        let mut accounts = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, region, project_id, organization, created_at
                 FROM cloud_account ORDER BY name COLLATE NOCASE",
            )?;
            let rows = stmt.query_map([], Self::row_to_cloud_account)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for a in &mut accounts {
            a.has_secret = secrets::has_secret(&secrets::cloud_account_key(&a.id));
        }
        Ok(accounts)
    }

    pub fn get_cloud_account(&self, id: &str) -> Result<Option<CloudAccount>> {
        let mut account = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, region, project_id, organization, created_at
                 FROM cloud_account WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map([id], Self::row_to_cloud_account)?;
            match rows.next() {
                Some(v) => Some(v?),
                None => None,
            }
        };
        if let Some(a) = &mut account {
            a.has_secret = secrets::has_secret(&secrets::cloud_account_key(&a.id));
        }
        Ok(account)
    }

    pub fn upsert_cloud_account(&self, input: &CloudAccountInput) -> Result<CloudAccount> {
        let id = input
            .id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO cloud_account (id, name, kind, region, project_id, organization)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    kind = excluded.kind,
                    region = excluded.region,
                    project_id = excluded.project_id,
                    organization = excluded.organization",
                params![
                    id,
                    input.name,
                    input.kind,
                    input.region,
                    input.project_id,
                    input.organization,
                ],
            )?;
        }
        Ok(self.get_cloud_account(&id)?.expect("cloud account just upserted"))
    }

    pub fn delete_cloud_account(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM cloud_account WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ----- Agent conversations -----

    pub fn list_agent_conversations(&self, limit: i64) -> Result<Vec<AgentConversationMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, summary, updated_at
             FROM agent_conversation
             ORDER BY updated_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(AgentConversationMeta {
                id: r.get(0)?,
                title: r.get(1)?,
                summary: r.get(2)?,
                updated_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_agent_conversation(&self, id: &str) -> Result<Option<AgentConversation>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, summary, targets_json, messages_json, created_at, updated_at
             FROM agent_conversation WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(AgentConversation {
                id: r.get(0)?,
                title: r.get(1)?,
                summary: r.get(2)?,
                targets_json: r.get(3)?,
                messages_json: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    }

    pub fn upsert_agent_conversation(
        &self,
        input: &AgentConversationInput,
    ) -> Result<AgentConversation> {
        let parsed: Vec<ChatMessage> = serde_json::from_str(&input.messages_json).unwrap_or_default();
        let title = input
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| conversations::derive_title(&parsed));
        let summary = conversations::compact_summary(&parsed);
        let summary_opt = if summary.is_empty() {
            None
        } else {
            Some(summary)
        };
        let targets_json = if input.targets.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&input.targets).unwrap_or_else(|_| "[]".into()))
        };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_conversation (id, title, summary, targets_json, messages_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                summary = excluded.summary,
                targets_json = excluded.targets_json,
                messages_json = excluded.messages_json,
                updated_at = datetime('now')",
            params![
                input.id,
                title,
                summary_opt,
                targets_json,
                input.messages_json,
            ],
        )?;
        drop(conn);
        Ok(self
            .get_agent_conversation(&input.id)?
            .expect("conversation just upserted"))
    }

    pub fn delete_agent_conversation(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM agent_conversation WHERE id = ?1", params![id])?;
        Ok(())
    }
}

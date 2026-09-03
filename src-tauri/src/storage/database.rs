use std::sync::{Arc, Mutex};

use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

use super::models::{
    AgentApproval, AgentConversation, AgentConversationInput, AgentConversationMeta,
    AgentLogEntry, AgentPlan, AgentPlanMeta, AiProvider, AiProviderInput, AuthType,
    ChannelUnread, CloudAccount, CloudAccountInput, CloudflareAuditLog,
    CloudflareAuditLogInput, CronJob, CronJobInput, FeatureProposal, GoalSession, InfraProject,
    InfraProjectInput, KnownHost, Vps, VpsInput, VpsLoginPatch, Workspace, WorkspaceInput,
};
use crate::artifacts::Artifact;
use crate::ai::conversations;
use crate::ai::edits::EditRecord;
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
    ///
    /// Carries what was offered as well as what was pinned. Without both there is no way
    /// to tell a rebuilt server from an actual attack: the check the user has to make is
    /// "does `offered` equal what the server really presents", and they cannot make it if
    /// the app only tells them the value that is now wrong.
    Mismatch {
        expected: String,
        offered: String,
        key_type: String,
    },
}

fn workspace_payload_eq(existing: &Workspace, input: &WorkspaceInput) -> bool {
    existing.name == input.name
        && existing.viewport_json == input.viewport_json
        && existing.layout_mode == input.layout_mode
        && existing.nodes_json == input.nodes_json
        && existing.color == input.color
        && existing.icon == input.icon
        && existing.color_mode == input.color_mode
        && existing.project_json == input.project_json
}

/// One thing a human asked for from a chat app.
///
/// The in-memory turn context is cleared the instant the remote turn returns, which is
/// long before anything it delegated finishes. This row is what outlives that — and a
/// restart — so a task that ends an hour later still knows whose question it was and
/// which chat to answer in, rather than the last chat anybody happened to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRequest {
    pub id: String,
    /// Transport name as `Kind::as_str` writes it.
    pub transport: String,
    pub chat_id: String,
    pub author_id: String,
    pub message_id: Option<String>,
    /// The agent that answered, when the bridge names one.
    pub persona_id: Option<String>,
    /// What they asked, in their words.
    pub ask: String,
    /// open | answered | abandoned
    pub status: String,
    pub created_at: Option<String>,
    pub closed_at: Option<String>,
}

/// A message the app owes a human, durable until it is delivered or provably cannot be.
///
/// Delivery is at-least-once on purpose: a duplicate notification is a small annoyance,
/// and the failure being fixed is a person who was told "I will get back to you" and
/// never heard anything again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxMessage {
    pub id: String,
    pub request_id: Option<String>,
    pub goal_id: Option<String>,
    pub transport: String,
    pub chat_id: String,
    pub body: String,
    /// pending | sending | sent | dead
    pub state: String,
    pub attempts: i64,
    /// When it may next be tried, and — once claimed — when it was claimed.
    pub next_attempt_at: Option<String>,
    pub last_error: Option<String>,
    /// One row per logical event. The uniqueness is the whole debounce.
    pub dedupe_key: String,
    pub created_at: Option<String>,
    pub sent_at: Option<String>,
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
            last_data_version: Mutex::new(None),
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
            last_data_version: Mutex::new(None),
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
            last_data_version: Mutex::new(None),
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

    /// Re-lock a running, unlocked encrypted DB **without quitting**: flush to the encrypted
    /// blob, stop the persister, swap the live connection back to an empty in-memory
    /// placeholder, and delete the plaintext working file.
    ///
    /// This is the only path that removes the decrypted database from disk while the app is
    /// still running. Ordering is the whole point:
    ///
    /// 1. persist **first** — anything not yet flushed would otherwise be lost, and a lock
    ///    that silently discards the last few seconds of work would teach users to avoid it;
    /// 2. swap the connection, which drops the old one and closes the file — Windows refuses
    ///    to delete a file that is still open, so cleanup before this point is a silent no-op;
    /// 3. only then delete the plaintext.
    ///
    /// Returns `Ok` and does nothing for a DB that is not encrypted — there is no plaintext
    /// to remove and no key to forget.
    pub fn relock(&self) -> Result<()> {
        let ctx = self.persist.lock().unwrap().clone();
        let Some(ctx) = ctx else { return Ok(()) };

        // 1. Flush. Do this before anything is torn down, and propagate failure: locking
        //    is not worth losing data over, and the caller can surface it.
        super::encrypt::persist_now(&self.conn, &ctx)?;

        ctx.stopped.store(true, std::sync::atomic::Ordering::Release);
        {
            let conn = self.conn.lock().unwrap();
            conn.commit_hook::<fn() -> bool>(None);
        }

        // 2. Swap. Assigning drops the previous Connection, closing the plaintext file.
        {
            let mut guard = self.conn.lock().unwrap();
            *guard = Connection::open_in_memory()?;
        }
        *self.persist.lock().unwrap() = None;
        // The placeholder needs the schema, or every query answers "no such table" instead
        // of the empty result the locked UI expects.
        self.migrate()?;

        // 3. Delete the plaintext, now that nothing holds it open.
        super::encrypt::cleanup_work_files(&ctx.work);
        Ok(())
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

    #[cfg(test)]
    fn is_placeholder(&self) -> bool {
        !self.is_encrypted()
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
                -- Per key type, like OpenSSH's known_hosts: a server publishes an
                -- ed25519, an RSA and an ECDSA key, and which one it presents depends on
                -- algorithm negotiation. Pinning only one of them per host turns a change
                -- of negotiated algorithm into a MITM accusation.
                PRIMARY KEY (host, port, key_type)
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

            -- Persistent autonomous goal sessions (/goal).
            CREATE TABLE IF NOT EXISTS goal_sessions (
                id            TEXT PRIMARY KEY,
                title         TEXT NOT NULL,
                raw_request   TEXT NOT NULL,
                spec_json     TEXT NOT NULL,
                status        TEXT NOT NULL DEFAULT 'intake',
                kanban_json   TEXT NOT NULL DEFAULT '[]',
                memory_json   TEXT NOT NULL DEFAULT '{}',
                next_check_at TEXT,
                cycles        INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
                finished_at   TEXT
            );

            -- Named agents the user can hand work to. A persona is an identity the
            -- goal loop runs under: its own instructions, default servers, safety
            -- mode, and optionally its own provider so cheap grunt work and
            -- architectural judgement do not have to share one model.
            CREATE TABLE IF NOT EXISTS persona (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                role          TEXT NOT NULL DEFAULT '',
                instructions  TEXT NOT NULL DEFAULT '',
                -- JSON array of vps ids this persona works on by default.
                targets_json  TEXT NOT NULL DEFAULT '[]',
                -- Overrides the global safety mode when set ('full'|'allowlist'|'approve').
                safety_mode   TEXT,
                -- Overrides the active provider/model when set.
                provider_id   TEXT,
                model         TEXT,
                enabled       INTEGER NOT NULL DEFAULT 1,
                -- The persona this one reports to. NULL = reports to the user.
                -- Forms the org chart; only a persona with no manager may address
                -- the user directly, everyone else escalates upward.
                reports_to    TEXT REFERENCES persona(id) ON DELETE SET NULL,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- What the agents say to each other.
            --
            -- Kept as rows rather than buried in each goal's transcript so the whole
            -- exchange can be read as one conversation: who asked whom for what, who
            -- reported back, and what finally reached the user.
            -- What a project is actually producing.
            --
            -- The teams exist to make the projects earn, and without figures that is
            -- unmeasurable: an agent can report that it shipped three fixes and still
            -- have no idea whether anything got better. One row is one number for one
            -- day, so periods can be compared — which is the only way "sales are down"
            -- becomes a fact rather than a feeling.
            CREATE TABLE IF NOT EXISTS project_metric (
                id           TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                -- 'revenue', 'orders', 'signups', 'refunds' — whatever the project has.
                name         TEXT NOT NULL,
                -- The day the figure is *about*, not when it was written: a number
                -- recorded late still belongs to its own day.
                period       TEXT NOT NULL,
                value        REAL NOT NULL,
                unit         TEXT,
                note         TEXT,
                -- Persona that recorded it. NULL = the user.
                source_id    TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                -- Re-recording a day corrects it instead of double-counting. Agents
                -- re-read the same source, and a duplicated day would quietly invent
                -- revenue.
                UNIQUE(workspace_id, name, period)
            );
            CREATE INDEX IF NOT EXISTS idx_project_metric_lookup
                ON project_metric (workspace_id, name, period);

            -- How to fetch a number, so it does not have to be fetched by hand.
            --
            -- A figure nobody records is a figure nobody has, and asking a person to
            -- type in yesterday's revenue every morning means it stops happening by
            -- Thursday. One command per metric per project, defined once, run on a
            -- schedule: whatever prints the number — a SQL query, a log count, an API
            -- call — is the same shape from here.
            CREATE TABLE IF NOT EXISTS metric_source (
                id           TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                name         TEXT NOT NULL,
                -- Server it runs on.
                vps_id       TEXT NOT NULL,
                -- Must print one number on stdout. Refused unless read-only: this runs
                -- unattended forever after one approval, and a metric that changes
                -- something is not a measurement.
                command      TEXT NOT NULL,
                unit         TEXT,
                enabled      INTEGER NOT NULL DEFAULT 1,
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(workspace_id, name)
            );

            CREATE TABLE IF NOT EXISTS agent_message (
                id           TEXT PRIMARY KEY,
                -- Sender. NULL = the user.
                from_id      TEXT,
                -- Recipient. NULL = the user.
                to_id        TEXT,
                -- 'report' (upward), 'request' (downward/sideways), 'note' (broadcast).
                kind         TEXT NOT NULL DEFAULT 'note',
                body         TEXT NOT NULL,
                -- The delegated task this concerns, when there is one.
                goal_id      TEXT,
                -- The project this was said about. NULL for messages from before
                -- projects existed, and for anything genuinely cross-project.
                workspace_id TEXT,
                -- Set once the recipient has been shown it.
                read_at      TEXT,
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS agent_message_to_idx
                ON agent_message(to_id, read_at);

            -- Per-reader position in a channel. A single `read_at` on the message says
            -- "the addressee saw it", which cannot answer "has Ada read #general" for
            -- six readers of the same room.
            -- reader_id is '' rather than NULL for the user: SQLite permits NULL in a
            -- primary key, which would silently duplicate the user's cursor on every
            -- upsert instead of conflicting.
            CREATE TABLE IF NOT EXISTS channel_read (
                reader_id    TEXT NOT NULL DEFAULT '',
                channel_id   TEXT NOT NULL,
                last_read_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (reader_id, channel_id)
            );

            -- What an agent actually did, durably. The live status event is
            -- fire-and-forget with a 120s client-side TTL, so a per-agent log channel
            -- built on it is empty after a restart and invisible to every other agent.
            CREATE TABLE IF NOT EXISTS agent_log (
                id           TEXT PRIMARY KEY,
                persona_id   TEXT NOT NULL,
                workspace_id TEXT,
                goal_id      TEXT,
                session_id   TEXT NOT NULL DEFAULT '',
                status       TEXT NOT NULL,
                tool         TEXT,
                detail       TEXT NOT NULL DEFAULT '',
                created_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_agent_log_persona
                ON agent_log (persona_id, created_at);

            -- A new feature an agent wants to build, and how far up the chain it has
            -- got. Improvements to what exists are autonomous; this is the gate that
            -- makes "new" different, in data rather than in prompt text.
            CREATE TABLE IF NOT EXISTS feature_proposal (
                id            TEXT PRIMARY KEY,
                workspace_id  TEXT,
                persona_id    TEXT,
                goal_id       TEXT,
                title         TEXT NOT NULL,
                body          TEXT NOT NULL,
                -- proposed | at_ceo | at_orchestrator | approved | rejected
                state         TEXT NOT NULL DEFAULT 'proposed',
                decided_by    TEXT,
                decision_note TEXT,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_feature_proposal_state
                ON feature_proposal (state, workspace_id);

            -- Who asked, on which transport, in which chat. The in-memory turn context
            -- is cleared the instant the remote turn returns -- long before the work it
            -- delegated finishes -- so without this row there is nobody to report back
            -- to and the task ends in silence.
            CREATE TABLE IF NOT EXISTS remote_request (
                id         TEXT PRIMARY KEY,
                transport  TEXT NOT NULL,
                chat_id    TEXT NOT NULL,
                author_id  TEXT NOT NULL DEFAULT '',
                message_id TEXT,
                persona_id TEXT,
                ask        TEXT NOT NULL,
                -- open | answered | abandoned
                status     TEXT NOT NULL DEFAULT 'open',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                closed_at  TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_remote_request_status
                ON remote_request (status, created_at);

            -- Outbound messages the app owes a human. Durable because the failure this
            -- fixes is silence: an in-memory queue loses the answer on the restart that
            -- is most likely to happen while a long task runs.
            CREATE TABLE IF NOT EXISTS remote_outbox (
                id              TEXT PRIMARY KEY,
                request_id      TEXT,
                goal_id         TEXT,
                transport       TEXT NOT NULL,
                chat_id         TEXT NOT NULL,
                body            TEXT NOT NULL,
                -- pending | sending | sent | dead
                state           TEXT NOT NULL DEFAULT 'pending',
                attempts        INTEGER NOT NULL DEFAULT 0,
                next_attempt_at TEXT,
                last_error      TEXT,
                -- One delivery per logical event. Delivery is deliberately
                -- at-least-once: a rare duplicate beats the silence being fixed.
                dedupe_key      TEXT NOT NULL UNIQUE,
                created_at      TEXT NOT NULL DEFAULT (datetime('now')),
                sent_at         TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_remote_outbox_due
                ON remote_outbox (state, next_attempt_at);

            -- The pull requests an agent has open. Keyed on persona because that is
            -- what every other agent-scoped table keys on.
            CREATE TABLE IF NOT EXISTS agent_open_pr (
                persona_id   TEXT NOT NULL,
                workspace_id TEXT NOT NULL DEFAULT '',
                branch       TEXT NOT NULL,
                pr_number    INTEGER,
                url          TEXT,
                opened_at    TEXT NOT NULL DEFAULT (datetime('now')),
                closed_at    TEXT,
                PRIMARY KEY (persona_id, workspace_id, branch)
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

            -- A remembered database login. The password is NOT here: it lives in the OS
            -- keychain under `db:<id>:password`, so it inherits the "Encrypt saved
            -- credentials" setting like every other secret.
            CREATE TABLE IF NOT EXISTS db_connection (
                id          TEXT PRIMARY KEY,
                vps_id      TEXT NOT NULL,
                -- Discovery's endpoint id (`docker:<name>` / `native:<port>`), so a saved
                -- login can be matched back to an instance on the next scan.
                endpoint_id TEXT NOT NULL,
                engine      TEXT NOT NULL,
                host        TEXT NOT NULL,
                port        INTEGER NOT NULL,
                container   TEXT,
                username    TEXT NOT NULL,
                database    TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE (vps_id, endpoint_id, username)
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

            -- Plans the agent presents via present_plan, for review history.
            -- status: presented | applied | archived | cancelled
            CREATE TABLE IF NOT EXISTS agent_plan (
                id           TEXT PRIMARY KEY,
                session_id   TEXT NOT NULL,
                workspace_id TEXT,
                title        TEXT,
                plan         TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'presented',
                created_at   TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Files the agent changed (before/after), for per-session and
            -- per-workspace diff history.
            CREATE TABLE IF NOT EXISTS agent_file_change (
                id           TEXT PRIMARY KEY,
                session_id   TEXT NOT NULL,
                workspace_id TEXT,
                scope        TEXT NOT NULL,
                vps_id       TEXT,
                label        TEXT,
                path         TEXT NOT NULL,
                before       TEXT,
                after        TEXT,
                is_new       INTEGER NOT NULL DEFAULT 0,
                reverted     INTEGER NOT NULL DEFAULT 0,
                ts           INTEGER NOT NULL
            );

            -- Local files the agent created (SSH key backups, downloads, writes).
            -- Secret rows are listed by path/hash only; contents are never served to tools.
            CREATE TABLE IF NOT EXISTS artifact (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                path        TEXT NOT NULL,
                kind        TEXT NOT NULL,
                sha256      TEXT NOT NULL,
                size        INTEGER NOT NULL DEFAULT 0,
                secret      INTEGER NOT NULL DEFAULT 0,
                session_id  TEXT,
                vps_id      TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Audit log of changes made to Cloudflare with before/after state for instant rollback.
            CREATE TABLE IF NOT EXISTS cloudflare_audit_log (
                id            TEXT PRIMARY KEY,
                account_id    TEXT NOT NULL,
                action_type   TEXT NOT NULL,
                target_id     TEXT,
                target_name   TEXT,
                summary       TEXT NOT NULL,
                actor         TEXT NOT NULL,
                session_id    TEXT,
                before_state  TEXT,
                after_state   TEXT,
                reverted      INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL DEFAULT (datetime('now')),
                ts            INTEGER NOT NULL
            );
            "#,
        )?;

        // Add columns to pre-existing databases (ignore "duplicate column" errors).
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN color TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN icon TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN color_mode TEXT", []);
        let _ = conn.execute("ALTER TABLE workspace ADD COLUMN project_json TEXT", []);
        let _ = conn.execute("ALTER TABLE vps ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0", []);
        // Which persona is running this goal. NULL = the default agent, which is what
        // every goal created before personas existed was.
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN persona_id TEXT", []);
        let _ = conn.execute("ALTER TABLE persona ADD COLUMN reports_to TEXT", []);
        // Scope the council to a project. Existing rows keep NULL: they were said
        // before there was a project to say them about, and guessing one retroactively
        // would file real history under a workspace it may have nothing to do with.
        let _ = conn.execute("ALTER TABLE agent_message ADD COLUMN workspace_id TEXT", []);
        // One team per project. Existing agents stay company-wide (NULL) rather than
        // being assigned to whichever project happens to be open — an agent silently
        // acquiring a home would change who routing picks.
        let _ = conn.execute("ALTER TABLE persona ADD COLUMN workspace_id TEXT", []);
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN workspace_id TEXT", []);
        // What came of the task, not just that it ended.
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN outcome TEXT", []);
        // Which room a message was said in, and which message it answers. Both NULL on
        // every row written before channels existed: those are routed by the client's
        // original derivation, so upgrading does not hide the existing history.
        let _ = conn.execute("ALTER TABLE agent_message ADD COLUMN channel_id TEXT", []);
        let _ = conn.execute("ALTER TABLE agent_message ADD COLUMN parent_id TEXT", []);
        let _ = conn.execute("ALTER TABLE agent_message ADD COLUMN mentions_json TEXT", []);
        // read_at means "delivered into a prompt" and is set on delivery, so a bug
        // report is shown once and then looks handled whether or not it was. Resolving
        // one is a separate, evidenced act.
        let _ = conn.execute("ALTER TABLE agent_message ADD COLUMN resolved_at TEXT", []);
        // Where an agent may write, and which tools it may call. NULL = the project
        // root / every tool, which is what every persona created before this had.
        let _ = conn.execute("ALTER TABLE persona ADD COLUMN allowed_paths TEXT", []);
        let _ = conn.execute("ALTER TABLE persona ADD COLUMN allowed_tools TEXT", []);
        // The chat that asked for this work, so finishing it can reach them.
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN request_id TEXT", []);
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN reported_at TEXT", []);
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN pr_number INTEGER", []);
        let _ = conn.execute("ALTER TABLE goal_sessions ADD COLUMN approval_state TEXT", []);
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_message_channel
             ON agent_message (channel_id, created_at)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_message_parent
             ON agent_message (parent_id, created_at)",
            [],
        );
        // A schedule that is a member of staff rather than a script: it runs as an
        // agent, on a project.
        let _ = conn.execute("ALTER TABLE cron_job ADD COLUMN workspace_id TEXT", []);
        let _ = conn.execute("ALTER TABLE cron_job ADD COLUMN persona_id TEXT", []);
        // The inbox and the per-project history are both read on every agent cycle.
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_message_workspace
             ON agent_message (workspace_id, created_at)",
            [],
        );
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN backend TEXT DEFAULT 'vps'", []);
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN cloud_account_id TEXT", []);
        let _ = conn.execute("ALTER TABLE infra_project ADD COLUMN config_json TEXT", []);

        // known_host used to be keyed on (host, port) alone, so a host could hold exactly
        // one pinned key. An SSH server publishes several — ed25519, RSA, ECDSA — and
        // which one it presents is decided by algorithm negotiation, i.e. by the client's
        // preference order. Change that order (a russh update is enough) and the server
        // offers a different one of its own keys, whose fingerprint cannot match the
        // pinned one: a permanent "host key mismatch (possible MITM)" for a server that
        // never changed. OpenSSH keys known_hosts by key type for exactly this reason.
        //
        // SQLite cannot alter a primary key, so rebuild the table. Existing pins are kept
        // — they are still true for the key type they were recorded against.
        let needs_rekey = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('known_host') l
                   JOIN pragma_index_info(l.name) i
                  WHERE l.origin = 'pk' AND i.name = 'key_type'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            == 0;
        if needs_rekey {
            let _ = conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS known_host_v2 (
                     host        TEXT NOT NULL,
                     port        INTEGER NOT NULL,
                     key_type    TEXT NOT NULL,
                     fingerprint TEXT NOT NULL,
                     added_at    TEXT NOT NULL DEFAULT (datetime('now')),
                     PRIMARY KEY (host, port, key_type)
                 );
                 INSERT OR IGNORE INTO known_host_v2 (host, port, key_type, fingerprint, added_at)
                     SELECT host, port, key_type, fingerprint, added_at FROM known_host;
                 DROP TABLE known_host;
                 ALTER TABLE known_host_v2 RENAME TO known_host;
                 COMMIT;",
            );
        }
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

    /// Update only the public login fields. Passwords / private keys are never read or written.
    pub fn patch_vps_login(&self, id: &str, patch: &VpsLoginPatch) -> Result<Vps> {
        let current = self
            .get_vps(id)?
            .ok_or_else(|| anyhow::anyhow!("VPS not found"))?;
        let input = VpsInput {
            id: Some(current.id),
            name: patch.name.clone().unwrap_or(current.name),
            host: patch.host.clone().unwrap_or(current.host),
            port: patch.port.unwrap_or(current.port),
            username: patch.username.clone().unwrap_or(current.username),
            auth_type: patch.auth_type.clone().unwrap_or(current.auth_type),
            key_path: match &patch.key_path {
                Some(p) if p.is_empty() => None,
                Some(p) => Some(p.clone()),
                None => current.key_path,
            },
            tags: current.tags,
            secret: None,
        };
        self.upsert_vps(&input)
    }

    pub fn insert_artifact(&self, art: &Artifact) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO artifact (id, name, path, kind, sha256, size, secret, session_id, vps_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                art.id,
                art.name,
                art.path,
                art.kind,
                art.sha256,
                art.size as i64,
                if art.secret { 1 } else { 0 },
                art.session_id,
                art.vps_id,
            ],
        )?;
        Ok(())
    }

    pub fn list_artifacts(&self, query: Option<&str>) -> Result<Vec<Artifact>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, kind, sha256, size, secret, session_id, vps_id, created_at
             FROM artifact ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Artifact {
                id: r.get(0)?,
                name: r.get(1)?,
                path: r.get(2)?,
                kind: r.get(3)?,
                sha256: r.get(4)?,
                size: r.get::<_, i64>(5)? as u64,
                secret: r.get::<_, i64>(6)? != 0,
                session_id: r.get(7)?,
                vps_id: r.get(8)?,
                created_at: r.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        let q = query.unwrap_or("").trim().to_ascii_lowercase();
        for row in rows {
            let a = row?;
            if q.is_empty()
                || a.name.to_ascii_lowercase().contains(&q)
                || a.path.to_ascii_lowercase().contains(&q)
                || a.kind.to_ascii_lowercase().contains(&q)
                || a.sha256.to_ascii_lowercase().contains(&q)
            {
                out.push(a);
            }
        }
        Ok(out)
    }

    pub fn get_artifact(&self, id: &str) -> Result<Option<Artifact>> {
        Ok(self.list_artifacts(None)?.into_iter().find(|a| a.id == id))
    }

    pub fn delete_artifact(&self, id: &str) -> Result<Option<Artifact>> {
        let existing = self.get_artifact(id)?;
        if existing.is_some() {
            let conn = self.conn.lock().unwrap();
            conn.execute("DELETE FROM artifact WHERE id = ?1", [id])?;
        }
        Ok(existing)
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
        // An unchanged layout still used to bump `updated_at`, which marked the
        // encrypted DB dirty and rewrote the whole ciphertext every autosave tick.
        if input.id.is_some() {
            if let Some(existing) = self.get_workspace(&id)? {
                if workspace_payload_eq(&existing, input) {
                    return Ok(existing);
                }
            }
        }
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
            workspace_id: r.get(10)?,
            persona_id: r.get(11)?,
        })
    }

    pub fn list_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, kind, payload, targets_json, enabled, last_run, last_status, created_at,
                    workspace_id, persona_id
             FROM cron_job ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], Self::row_to_cron)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, kind, payload, targets_json, enabled, last_run, last_status, created_at,
                    workspace_id, persona_id
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
                "INSERT INTO cron_job (id, name, schedule, kind, payload, targets_json, enabled,
                                       workspace_id, persona_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    schedule = excluded.schedule,
                    kind = excluded.kind,
                    payload = excluded.payload,
                    targets_json = excluded.targets_json,
                    enabled = excluded.enabled,
                    workspace_id = excluded.workspace_id,
                    persona_id = excluded.persona_id",
                params![
                    id,
                    input.name,
                    input.schedule,
                    input.kind,
                    input.payload,
                    input.targets_json,
                    input.enabled as i64,
                    input.workspace_id.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                    input.persona_id.as_deref().map(str::trim).filter(|s| !s.is_empty()),
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

    // ----- Goal sessions (/goal) -----

    fn row_to_goal(r: &rusqlite::Row) -> rusqlite::Result<GoalSession> {
        Ok(GoalSession {
            id: r.get(0)?,
            title: r.get(1)?,
            raw_request: r.get(2)?,
            spec_json: r.get(3)?,
            status: r.get(4)?,
            kanban_json: r.get(5)?,
            memory_json: r.get(6)?,
            next_check_at: r.get(7)?,
            cycles: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
            finished_at: r.get(11)?,
            persona_id: r.get(12)?,
            workspace_id: r.get(13)?,
            outcome: r.get(14)?,
            request_id: r.get(15)?,
            reported_at: r.get(16)?,
            pr_number: r.get(17)?,
            approval_state: r.get(18)?,
        })
    }

    pub fn list_goals(&self) -> Result<Vec<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], Self::row_to_goal)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_goal(&self, id: &str) -> Result<Option<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_goal(row)?)),
            None => Ok(None),
        }
    }

    /// Insert a new goal session in "intake" status.
    pub fn insert_goal(&self, goal: &GoalSession) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO goal_sessions
               (id, title, raw_request, spec_json, status, kanban_json, memory_json, next_check_at, cycles, persona_id, workspace_id, request_id, approval_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                goal.id,
                goal.title,
                goal.raw_request,
                goal.spec_json,
                goal.status,
                goal.kanban_json,
                goal.memory_json,
                goal.next_check_at,
                goal.cycles,
                goal.persona_id,
                goal.workspace_id,
                goal.request_id,
                goal.approval_state,
            ],
        )?;
        Ok(())
    }

    pub fn update_goal(&self, goal: &GoalSession) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE goal_sessions SET
                title = ?2, raw_request = ?3, spec_json = ?4, status = ?5,
                kanban_json = ?6, memory_json = ?7, next_check_at = ?8, cycles = ?9,
                updated_at = datetime('now'), finished_at = ?10, outcome = ?11,
                reported_at = ?12, pr_number = ?13, approval_state = ?14
             WHERE id = ?1",
            params![
                goal.id,
                goal.title,
                goal.raw_request,
                goal.spec_json,
                goal.status,
                goal.kanban_json,
                goal.memory_json,
                goal.next_check_at,
                goal.cycles,
                goal.finished_at,
                goal.outcome,
                goal.reported_at,
                goal.pr_number,
                goal.approval_state,
            ],
        )?;
        Ok(())
    }

    // ----- Personas (named background agents) -----

    fn row_to_persona(r: &rusqlite::Row) -> rusqlite::Result<crate::storage::models::Persona> {
        let targets_json: String = r.get(4)?;
        Ok(crate::storage::models::Persona {
            id: r.get(0)?,
            name: r.get(1)?,
            role: r.get(2)?,
            instructions: r.get(3)?,
            // A malformed targets blob must not make the persona unreadable — an
            // unusable persona row is worse than one that has forgotten its defaults.
            targets: serde_json::from_str(&targets_json).unwrap_or_default(),
            safety_mode: r.get(5)?,
            provider_id: r.get(6)?,
            model: r.get(7)?,
            enabled: r.get::<_, i64>(8)? != 0,
            reports_to: r.get(9)?,
            created_at: r.get(10)?,
            updated_at: r.get(11)?,
            workspace_id: r.get(12)?,
            allowed_paths: r
                .get::<_, Option<String>>(13)?
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
            allowed_tools: r
                .get::<_, Option<String>>(14)?
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        })
    }

    /// Append-only: `row_to_persona` reads by position.
    const PERSONA_COLS: &'static str =
        "id, name, role, instructions, targets_json, safety_mode, provider_id, model,
         enabled, reports_to, created_at, updated_at, workspace_id, allowed_paths,
         allowed_tools";

    pub fn list_personas(&self) -> Result<Vec<crate::storage::models::Persona>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {} FROM persona ORDER BY name", Self::PERSONA_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::row_to_persona)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_persona(&self, id: &str) -> Result<Option<crate::storage::models::Persona>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {} FROM persona WHERE id = ?1", Self::PERSONA_COLS);
        match conn.query_row(&sql, [id], Self::row_to_persona) {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Find a persona by name, case-insensitively.
    ///
    /// The agent and the user refer to a persona by name ("ask Ada to…"), never by
    /// uuid, and neither will reliably match its capitalisation.
    pub fn get_persona_by_name(
        &self,
        name: &str,
    ) -> Result<Option<crate::storage::models::Persona>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM persona WHERE lower(name) = lower(?1)",
            Self::PERSONA_COLS
        );
        match conn.query_row(&sql, [name.trim()], Self::row_to_persona) {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn upsert_persona(
        &self,
        input: &crate::storage::models::PersonaInput,
    ) -> Result<crate::storage::models::Persona> {
        let id = input
            .id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let targets_json = serde_json::to_string(&input.targets).unwrap_or_else(|_| "[]".into());
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO persona
                   (id, name, role, instructions, targets_json, safety_mode, provider_id, model, enabled, reports_to, workspace_id, allowed_paths, allowed_tools)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   role = excluded.role,
                   instructions = excluded.instructions,
                   targets_json = excluded.targets_json,
                   safety_mode = excluded.safety_mode,
                   provider_id = excluded.provider_id,
                   model = excluded.model,
                   enabled = excluded.enabled,
                   reports_to = excluded.reports_to,
                   workspace_id = excluded.workspace_id,
                   allowed_paths = excluded.allowed_paths,
                   allowed_tools = excluded.allowed_tools,
                   updated_at = datetime('now')",
                params![
                    id,
                    input.name.trim(),
                    input.role.trim(),
                    input.instructions,
                    targets_json,
                    input.safety_mode,
                    input.provider_id,
                    input.model,
                    input.enabled as i64,
                    input.reports_to,
                    input.workspace_id.as_deref().map(str::trim).filter(|s| !s.is_empty()),
                    // NULL, not "[]": an empty list means "scoped to nothing", and the
                    // absence of a scope means the project root.
                    if input.allowed_paths.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&input.allowed_paths).ok()
                    },
                    if input.allowed_tools.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&input.allowed_tools).ok()
                    },
                ],
            )?;
        }
        Ok(self
            .get_persona(&id)?
            .expect("persona just upserted"))
    }

    pub fn delete_persona(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM persona WHERE id = ?1", [id])?;
        Ok(())
    }

    // ----- Inter-agent messages -----

    fn row_to_agent_message(
        r: &rusqlite::Row,
    ) -> rusqlite::Result<crate::storage::models::AgentMessage> {
        Ok(crate::storage::models::AgentMessage {
            id: r.get(0)?,
            from_id: r.get(1)?,
            to_id: r.get(2)?,
            kind: r.get(3)?,
            body: r.get(4)?,
            goal_id: r.get(5)?,
            workspace_id: r.get(6)?,
            read_at: r.get(7)?,
            created_at: r.get(8)?,
            channel_id: r.get(9)?,
            parent_id: r.get(10)?,
            // A malformed mentions blob must not make the message unreadable.
            mentions: r
                .get::<_, Option<String>>(11)?
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
            resolved_at: r.get(12)?,
        })
    }

    /// Append-only. The row mapper reads by position, so inserting a column in the
    /// middle silently shifts every field after it.
    const AGENT_MESSAGE_COLS: &'static str =
        "id, from_id, to_id, kind, body, goal_id, workspace_id, read_at, created_at, \
         channel_id, parent_id, mentions_json, resolved_at";

    pub fn insert_agent_message(
        &self,
        msg: &crate::storage::models::AgentMessage,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_message
               (id, from_id, to_id, kind, body, goal_id, workspace_id, channel_id,
                parent_id, mentions_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.from_id,
                msg.to_id,
                msg.kind,
                msg.body,
                msg.goal_id,
                msg.workspace_id,
                msg.channel_id,
                msg.parent_id,
                if msg.mentions.is_empty() {
                    None
                } else {
                    serde_json::to_string(&msg.mentions).ok()
                },
            ],
        )?;
        Ok(())
    }

    /// Unread messages addressed to `to_id` (None = the user's own inbox).
    ///
    /// `workspace` of `Some(id)` returns only what was said about that project. This is
    /// the difference between an agent reading its own project's thread and reading
    /// every project at once, which is unusable the moment there are two. `None` means
    /// no project is selected, and everything is in scope.
    /// Unread messages addressed to `to_id` (None = the user's own inbox).
    ///
    /// Everything addressed to you is delivered, whichever project it is about. Scoping
    /// this the way the *thread* is scoped looked right and was not: a lead writing to
    /// another project's lead had the message stamped with their own project, so it
    /// never appeared in the recipient's inbox and the two could not talk at all.
    ///
    /// The original complaint about mixed projects was not knowing which was which, so
    /// the answer is a label rather than a filter — see `agent_inbox`. Nothing addressed
    /// to somebody should be silently withheld from them.
    pub fn unread_agent_messages(
        &self,
        to_id: Option<&str>,
    ) -> Result<Vec<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        // `to_id IS ?1` rather than `=`, so NULL (the user) matches instead of
        // silently returning nothing the way SQL equality against NULL would.
        let sql = format!(
            // `channel_id IS NULL` keeps room posts out of the inbox: a channel post
            // carries no recipient, and `to_id IS NULL` already means "the user", so
            // without this every broadcast lands in the user's own unread pile.
            "SELECT {} FROM agent_message
             WHERE to_id IS ?1 AND read_at IS NULL AND channel_id IS NULL
             ORDER BY created_at",

            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![to_id], Self::row_to_agent_message)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The whole exchange, oldest first, so the UI can show it as one conversation.
    ///
    /// `goal_id` narrows it to a single delegated task; `workspace` narrows it to one
    /// project. Both are `NULL`-tolerant in SQL rather than branching in Rust, which is
    /// how the previous version grew two near-identical query paths.
    pub fn list_agent_messages(
        &self,
        goal_id: Option<&str>,
        workspace: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let sql = format!(
            "SELECT {} FROM agent_message
             WHERE (?1 IS NULL OR goal_id = ?1)
               AND (?2 IS NULL OR workspace_id = ?2)
             ORDER BY created_at DESC LIMIT ?3",
            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![goal_id, workspace, limit], Self::row_to_agent_message)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Newest-first in SQL so LIMIT keeps the *recent* end; reversed here so the
        // caller reads it as a conversation.
        Ok(rows.into_iter().rev().collect())
    }

    /// Delegated tasks belonging to one project, newest first.
    pub fn list_goals_for_workspace(&self, workspace: Option<&str>) -> Result<Vec<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions
             WHERE (?1 IS NULL OR workspace_id = ?1)
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![workspace], Self::row_to_goal)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn mark_agent_messages_read(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for id in ids {
            tx.execute(
                "UPDATE agent_message SET read_at = datetime('now') WHERE id = ?1",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ----- Rooms, threads and the durable agent log -----

    /// One room's messages, oldest last, top-level only.
    ///
    /// Replies are excluded rather than interleaved: a thread hangs off the message it
    /// answers, and a correction written three days later must not reappear at the
    /// bottom of the room as though it were a fresh remark.
    pub fn list_channel_messages(
        &self,
        channel_id: &str,
        limit: i64,
    ) -> Result<Vec<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let sql = format!(
            "SELECT {} FROM agent_message
             WHERE channel_id = ?1 AND parent_id IS NULL
             ORDER BY created_at DESC LIMIT ?2",
            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![channel_id, limit], Self::row_to_agent_message)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Newest-first in SQL so LIMIT keeps the recent end; reversed so the caller
        // reads it as a conversation.
        Ok(rows.into_iter().rev().collect())
    }

    /// Every reply hanging off one message or log line, oldest first.
    ///
    /// The parent may be an `agent_message` id or an `agent_log` id — a suggestion is
    /// attached to a specific action just as readily as to a specific sentence, and the
    /// two id spaces are both uuids, so one column serves both.
    pub fn list_thread(
        &self,
        parent_id: &str,
    ) -> Result<Vec<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM agent_message
             WHERE parent_id = ?1
             ORDER BY created_at",
            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![parent_id], Self::row_to_agent_message)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// One message by id, so a caller can check a parent exists before answering it.
    pub fn get_agent_message(
        &self,
        id: &str,
    ) -> Result<Option<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM agent_message WHERE id = ?1",
            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id], Self::row_to_agent_message)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    fn row_to_agent_log(r: &rusqlite::Row) -> rusqlite::Result<AgentLogEntry> {
        Ok(AgentLogEntry {
            id: r.get(0)?,
            persona_id: r.get(1)?,
            workspace_id: r.get(2)?,
            goal_id: r.get(3)?,
            session_id: r.get(4)?,
            status: r.get(5)?,
            tool: r.get(6)?,
            detail: r.get(7)?,
            created_at: r.get(8)?,
        })
    }

    const AGENT_LOG_COLS: &'static str =
        "id, persona_id, workspace_id, goal_id, session_id, status, tool, detail, created_at";

    /// Record one thing an agent did.
    ///
    /// `created_at` is left to the column default so every row in this table and in
    /// `agent_message` shares one timestamp format — read cursors compare as strings,
    /// and one caller writing RFC3339 while another writes `datetime('now')` would sort
    /// every mixed pair wrongly.
    pub fn insert_agent_log(&self, entry: &AgentLogEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_log
               (id, persona_id, workspace_id, goal_id, session_id, status, tool, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.id,
                entry.persona_id,
                entry.workspace_id,
                entry.goal_id,
                entry.session_id,
                entry.status,
                entry.tool,
                entry.detail,
            ],
        )?;
        Ok(())
    }

    /// What one agent has been doing, oldest last.
    pub fn list_agent_log(&self, persona_id: &str, limit: i64) -> Result<Vec<AgentLogEntry>> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 2000);
        let sql = format!(
            "SELECT {} FROM agent_log
             WHERE persona_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
            Self::AGENT_LOG_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![persona_id, limit], Self::row_to_agent_log)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().rev().collect())
    }

    /// Drop log lines older than `keep_days`, returning how many went.
    ///
    /// An agent emits a line per tool call, so this table grows faster than anything
    /// else here and is the one place where forgetting is the correct behaviour.
    pub fn prune_agent_log(&self, keep_days: i64) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let cutoff = format!("-{} days", keep_days.clamp(1, 3650));
        let n = conn.execute(
            "DELETE FROM agent_log WHERE created_at < datetime('now', ?1)",
            params![cutoff],
        )?;
        Ok(n)
    }

    /// Keep only the newest `keep` lines for one agent, dropping the rest.
    ///
    /// Age is the wrong measure for the write path: an agent that ran hard for an hour
    /// writes thousands of rows that are all minutes old, and the file is re-encrypted
    /// whole on lock. A count per agent bounds it where it actually grows, and
    /// `prune_agent_log` still handles the tail of agents nobody uses any more.
    pub fn trim_agent_log(&self, persona_id: &str, keep: i64) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM agent_log
              WHERE persona_id = ?1
                AND rowid NOT IN (
                    SELECT rowid FROM agent_log
                     WHERE persona_id = ?1
                     ORDER BY created_at DESC, rowid DESC
                     LIMIT ?2
                )",
            params![persona_id, keep.clamp(1, 10_000)],
        )?;
        Ok(n)
    }

    /// How much each room holds that `reader_id` has not seen. `""` is the user.
    ///
    /// Every room with messages comes back, read or not, and each row carries the
    /// reader's cursor: the client recounts against that cursor as live messages
    /// arrive, so a count taken at load does not go stale the moment somebody speaks.
    pub fn channel_unread_counts(&self, reader_id: &str) -> Result<Vec<ChannelUnread>> {
        let conn = self.conn.lock().unwrap();
        // The user is `from_id IS NULL`, a persona is `from_id = <id>`; `IS NOT` is
        // null-safe, so one comparison covers both and nobody's own words count as
        // unread to them.
        let author: Option<&str> = if reader_id.is_empty() {
            None
        } else {
            Some(reader_id)
        };
        // Mentions are stored as a JSON array of ids, so the id must be matched with
        // its quotes -- a bare substring would let one id match a longer one.
        let needle = if reader_id.is_empty() {
            String::new()
        } else {
            format!("\"{reader_id}\"")
        };
        let mut stmt = conn.prepare(
            "SELECT m.channel_id,
                    SUM(CASE WHEN (r.last_read_at IS NULL OR m.created_at > r.last_read_at)
                                  AND m.from_id IS NOT ?2
                             THEN 1 ELSE 0 END),
                    SUM(CASE WHEN (r.last_read_at IS NULL OR m.created_at > r.last_read_at)
                                  AND m.from_id IS NOT ?2
                                  AND ?3 <> ''
                                  AND m.mentions_json IS NOT NULL
                                  AND instr(m.mentions_json, ?3) > 0
                             THEN 1 ELSE 0 END),
                    r.last_read_at
             FROM agent_message m
             LEFT JOIN channel_read r
                    ON r.channel_id = m.channel_id AND r.reader_id = ?1
             WHERE m.channel_id IS NOT NULL
             GROUP BY m.channel_id, r.last_read_at
             ORDER BY m.channel_id",
        )?;
        let rows = stmt.query_map(params![reader_id, author, needle], |r| {
            Ok(ChannelUnread {
                channel_id: r.get(0)?,
                unread: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                mentions: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                last_read_at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Move a reader's cursor in a room to now.
    pub fn mark_channel_read(&self, reader_id: &str, channel_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        Self::upsert_channel_read(&conn, reader_id, channel_id, None)
    }

    /// Move a reader's cursor to an explicit point.
    ///
    /// An agent marks a room read as of the moment it was handed the messages, not the
    /// moment it finished thinking about them, or everything said while it worked is
    /// silently swallowed.
    #[allow(dead_code)]
    pub fn mark_channel_read_at(&self, reader_id: &str, channel_id: &str, at: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        Self::upsert_channel_read(&conn, reader_id, channel_id, Some(at))
    }

    fn upsert_channel_read(
        conn: &Connection,
        reader_id: &str,
        channel_id: &str,
        at: Option<&str>,
    ) -> Result<()> {
        // reader_id is '' for the user, never NULL: SQLite permits NULL in a primary
        // key, so a NULL reader would insert a fresh duplicate cursor row on every
        // upsert instead of conflicting, and the user's unread count would never clear.
        conn.execute(
            "INSERT INTO channel_read (reader_id, channel_id, last_read_at)
             VALUES (?1, ?2, COALESCE(?3, datetime('now')))
             ON CONFLICT(reader_id, channel_id)
             DO UPDATE SET last_read_at = COALESCE(?3, datetime('now'))",
            params![reader_id, channel_id, at],
        )?;
        Ok(())
    }

    // ----- Remote requests and the outbox -----
    //
    // Between a chat message and the work it starts there is a gap the process may not
    // survive, so both halves are rows rather than memory: who asked (and where they
    // are reachable), and what we still owe them.

    fn row_to_remote_request(r: &rusqlite::Row) -> rusqlite::Result<RemoteRequest> {
        Ok(RemoteRequest {
            id: r.get(0)?,
            transport: r.get(1)?,
            chat_id: r.get(2)?,
            author_id: r.get(3)?,
            message_id: r.get(4)?,
            persona_id: r.get(5)?,
            ask: r.get(6)?,
            status: r.get(7)?,
            created_at: r.get(8)?,
            closed_at: r.get(9)?,
        })
    }

    const REMOTE_REQUEST_COLS: &'static str =
        "id, transport, chat_id, author_id, message_id, persona_id, ask, status, \
         created_at, closed_at";

    /// Record who asked, before anything slow happens. The id it returns is what a
    /// finished task carries home.
    pub fn insert_remote_request(&self, r: &RemoteRequest) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            // `created_at` is normally the default. It is writable so that the sweep's
            // age rules can be exercised without waiting an hour for them.
            "INSERT INTO remote_request
               (id, transport, chat_id, author_id, message_id, persona_id, ask, status,
                created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, COALESCE(?9, datetime('now')))",
            params![
                r.id,
                r.transport,
                r.chat_id,
                r.author_id,
                r.message_id,
                r.persona_id,
                r.ask,
                r.status,
                r.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_remote_request(&self, id: &str) -> Result<Option<RemoteRequest>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM remote_request WHERE id = ?1",
            Self::REMOTE_REQUEST_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_remote_request(row)?)),
            None => Ok(None),
        }
    }

    /// Asks nobody has answered yet, oldest first. What the sweep reads.
    pub fn list_open_remote_requests(&self) -> Result<Vec<RemoteRequest>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM remote_request WHERE status = 'open' ORDER BY created_at",
            Self::REMOTE_REQUEST_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::row_to_remote_request)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Close an ask: `answered` when the reply went out, `abandoned` when it could not.
    ///
    /// Only an open request closes, so a late second delivery cannot reopen and re-stamp
    /// one that was already settled.
    pub fn close_remote_request(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE remote_request SET status = ?2, closed_at = datetime('now')
             WHERE id = ?1 AND status = 'open'",
            params![id, status],
        )?;
        Ok(())
    }

    /// The tasks started to answer one ask. `None` finds goals nobody is waiting on.
    pub fn goals_for_request(&self, request_id: &str) -> Result<Vec<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions WHERE request_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([request_id], Self::row_to_goal)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Stamp a goal with the ask it belongs to, once.
    ///
    /// Separate from `update_goal` on purpose: the loop rewrites a goal on every cycle
    /// from a struct it read minutes ago, and an ownership that arrived in between must
    /// not be written back out of it. `IS NULL` also makes this idempotent, so a task
    /// adopted by one ask is never re-parented by the next one.
    pub fn set_goal_request(&self, goal_id: &str, request_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE goal_sessions SET request_id = ?2 WHERE id = ?1 AND request_id IS NULL",
            params![goal_id, request_id],
        )?;
        Ok(n > 0)
    }

    fn row_to_outbox(r: &rusqlite::Row) -> rusqlite::Result<OutboxMessage> {
        Ok(OutboxMessage {
            id: r.get(0)?,
            request_id: r.get(1)?,
            goal_id: r.get(2)?,
            transport: r.get(3)?,
            chat_id: r.get(4)?,
            body: r.get(5)?,
            state: r.get(6)?,
            attempts: r.get(7)?,
            next_attempt_at: r.get(8)?,
            last_error: r.get(9)?,
            dedupe_key: r.get(10)?,
            created_at: r.get(11)?,
            sent_at: r.get(12)?,
        })
    }

    const OUTBOX_COLS: &'static str =
        "id, request_id, goal_id, transport, chat_id, body, state, attempts, \
         next_attempt_at, last_error, dedupe_key, created_at, sent_at";

    /// Queue one message. `false` means the same logical event was already queued —
    /// which is the point of the unique key, not a failure.
    pub fn enqueue_outbox(&self, m: &OutboxMessage) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "INSERT OR IGNORE INTO remote_outbox
               (id, request_id, goal_id, transport, chat_id, body, state, attempts,
                next_attempt_at, dedupe_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                m.id,
                m.request_id,
                m.goal_id,
                m.transport,
                m.chat_id,
                m.body,
                m.state,
                m.attempts,
                m.next_attempt_at,
                m.dedupe_key,
            ],
        )?;
        Ok(n > 0)
    }

    /// Read one queued message back. Used by the tests that prove a message is not sent
    /// twice and not dropped, which is the only place the worker's own state is visible.
    #[allow(dead_code)]
    pub fn get_outbox(&self, id: &str) -> Result<Option<OutboxMessage>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {} FROM remote_outbox WHERE id = ?1", Self::OUTBOX_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_outbox(row)?)),
            None => Ok(None),
        }
    }

    /// Everything queued, newest last. For diagnostics and tests.
    #[allow(dead_code)]
    pub fn list_outbox(&self, limit: i64) -> Result<Vec<OutboxMessage>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM remote_outbox ORDER BY created_at, rowid LIMIT ?1",
            Self::OUTBOX_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([limit.clamp(1, 1000)], Self::row_to_outbox)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Add to a message that has not gone out yet, so two agents finishing one ask
    /// arrive as one notification carrying both results rather than as one that has
    /// silently dropped the other.
    ///
    /// A no-op once the row is claimed: appending to a body already on its way to a
    /// phone would deliver half a sentence.
    pub fn append_outbox_body(&self, dedupe_key: &str, extra: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE remote_outbox SET body = substr(body || char(10) || char(10) || ?2, 1, 6000)
             WHERE dedupe_key = ?1 AND state = 'pending'",
            params![dedupe_key, extra],
        )?;
        Ok(n > 0)
    }

    /// Replace a queued body, while it is still queued.
    pub fn update_outbox_body(&self, id: &str, body: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE remote_outbox SET body = ?2 WHERE id = ?1 AND state = 'pending'",
            params![id, body],
        )?;
        Ok(n > 0)
    }

    /// Take the next due message, atomically.
    ///
    /// The claim is the `UPDATE ... WHERE state = 'pending'`: two workers racing on the
    /// same row means one of them changes no rows and gets nothing, which is what keeps
    /// a message from being sent twice. `next_attempt_at` is rewritten to now so it
    /// doubles as the claim time — a worker that dies mid-send leaves a row that
    /// `requeue_stalled_outbox` can date and return.
    pub fn claim_outbox(&self) -> Result<Option<OutboxMessage>> {
        let conn = self.conn.lock().unwrap();
        let id: Option<String> = {
            let mut stmt = conn.prepare(
                "SELECT id FROM remote_outbox
                 WHERE state = 'pending'
                   AND (next_attempt_at IS NULL
                        OR next_attempt_at <= strftime('%Y-%m-%d %H:%M:%S','now'))
                 ORDER BY created_at, rowid LIMIT 1",
            )?;
            let mut rows = stmt.query([])?;
            match rows.next()? {
                Some(r) => Some(r.get(0)?),
                None => None,
            }
        };
        let Some(id) = id else { return Ok(None) };
        let n = conn.execute(
            "UPDATE remote_outbox
               SET state = 'sending',
                   next_attempt_at = strftime('%Y-%m-%d %H:%M:%S','now')
             WHERE id = ?1 AND state = 'pending'",
            [&id],
        )?;
        if n == 0 {
            // Somebody else took it between the select and the update.
            return Ok(None);
        }
        let sql = format!("SELECT {} FROM remote_outbox WHERE id = ?1", Self::OUTBOX_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([&id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_outbox(row)?)),
            None => Ok(None),
        }
    }

    pub fn mark_outbox_sent(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE remote_outbox SET state = 'sent', sent_at = datetime('now'),
                    last_error = NULL
             WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// Record a failed attempt. `retry_at` of `None` gives up on the message.
    pub fn fail_outbox(&self, id: &str, error: &str, retry_at: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let state = if retry_at.is_some() { "pending" } else { "dead" };
        conn.execute(
            "UPDATE remote_outbox
               SET state = ?2, attempts = attempts + 1, last_error = ?3,
                   next_attempt_at = ?4
             WHERE id = ?1",
            params![id, state, error, retry_at],
        )?;
        Ok(())
    }

    /// Return anything claimed before `cutoff` and never finished.
    ///
    /// The crash case: a message marked `sending` by a process that then died is owed to
    /// somebody and will never move on its own.
    pub fn requeue_stalled_outbox(&self, cutoff: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE remote_outbox SET state = 'pending', next_attempt_at = NULL
             WHERE state = 'sending'
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?1)",
            [cutoff],
        )?;
        Ok(n)
    }

    /// Is anything already queued or delivered for this ask?
    ///
    /// `dead` deliberately does not count: a message that could not be delivered after
    /// every retry is not an answer, and the sweep should still be able to say so.
    pub fn outbox_live_for_request(&self, request_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM remote_outbox
             WHERE request_id = ?1 AND state IN ('pending','sending','sent')",
            [request_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Goals that are due to resume (status "waiting" with next_check_at <= now).
    pub fn list_due_goals(&self) -> Result<Vec<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions
             WHERE status = 'waiting' AND next_check_at IS NOT NULL
               AND next_check_at <= strftime('%Y-%m-%dT%H:%M:%SZ','now')
             ORDER BY next_check_at",
        )?;
        let rows = stmt.query_map([], Self::row_to_goal)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn delete_goal(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM goal_sessions WHERE id = ?1", [id])?;
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
        // Compared against the pin for THIS key type. Matching across types would compare
        // an ed25519 fingerprint with an RSA one and always disagree.
        let existing: Option<String> = conn
            .query_row(
                "SELECT fingerprint FROM known_host
                  WHERE host = ?1 AND port = ?2 AND key_type = ?3",
                params![host, port as i64, key_type],
                |r| r.get(0),
            )
            .ok();
        match existing {
            Some(expected) if expected == fingerprint => Ok(HostKeyVerdict::Match),
            Some(expected) => Ok(HostKeyVerdict::Mismatch {
                expected,
                offered: fingerprint.to_string(),
                key_type: key_type.to_string(),
            }),
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

    /// Remembered database logins for one server.
    pub fn list_db_connections(&self, vps_id: &str) -> Result<Vec<crate::storage::models::DbConnection>> {
        let mut list = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, vps_id, endpoint_id, engine, host, port, container, username, database
                 FROM db_connection WHERE vps_id = ?1 ORDER BY endpoint_id, username",
            )?;
            let rows = stmt.query_map([vps_id], |r| {
                Ok(crate::storage::models::DbConnection {
                    id: r.get(0)?,
                    vps_id: r.get(1)?,
                    endpoint_id: r.get(2)?,
                    engine: r.get(3)?,
                    host: r.get(4)?,
                    port: r.get::<_, i64>(5)? as u16,
                    container: r.get(6)?,
                    username: r.get(7)?,
                    database: r.get(8)?,
                    has_secret: false,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        // Checked outside the DB lock: each probe is an OS keychain round trip.
        for c in &mut list {
            c.has_secret = secrets::has_secret(&secrets::db_connection_key(&c.id));
        }
        Ok(list)
    }

    /// Insert or update a remembered login. Returns the row id.
    ///
    /// Keyed on (vps_id, endpoint_id, username) so re-saving the same login updates it
    /// rather than accumulating duplicates every time the user ticks "remember".
    pub fn upsert_db_connection(
        &self,
        c: &crate::storage::models::DbConnection,
    ) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO db_connection
                (id, vps_id, endpoint_id, engine, host, port, container, username, database)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (vps_id, endpoint_id, username) DO UPDATE SET
                engine = excluded.engine, host = excluded.host, port = excluded.port,
                container = excluded.container, database = excluded.database",
            params![
                c.id,
                c.vps_id,
                c.endpoint_id,
                c.engine,
                c.host,
                c.port as i64,
                c.container,
                c.username,
                c.database
            ],
        )?;
        // The conflict path keeps the pre-existing id, so read back whichever won —
        // otherwise the caller would store the password under an id that isn't in the row.
        let id: String = conn.query_row(
            "SELECT id FROM db_connection WHERE vps_id = ?1 AND endpoint_id = ?2 AND username = ?3",
            params![c.vps_id, c.endpoint_id, c.username],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn delete_db_connection(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM db_connection WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Every remembered login, for the secret re-key sweep.
    pub fn all_db_connection_ids(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM db_connection")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
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

    // ----- Cloudflare Audit & Rollback Logs -----

    pub fn create_cloudflare_audit_log(&self, input: &CloudflareAuditLogInput) -> Result<CloudflareAuditLog> {
        let id = Uuid::new_v4().to_string();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO cloudflare_audit_log (id, account_id, action_type, target_id, target_name, summary, actor, session_id, before_state, after_state, reverted, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
                params![
                    id,
                    input.account_id,
                    input.action_type,
                    input.target_id,
                    input.target_name,
                    input.summary,
                    input.actor,
                    input.session_id,
                    input.before_state,
                    input.after_state,
                    now_ts
                ],
            )?;
        }
        self.get_cloudflare_audit_log(&id)?
            .ok_or_else(|| anyhow::anyhow!("Cloudflare audit log not found immediately after insert"))
    }

    pub fn list_cloudflare_audit_logs(&self, account_id: &str, limit: u32) -> Result<Vec<CloudflareAuditLog>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, action_type, target_id, target_name, summary, actor, session_id, before_state, after_state, reverted, created_at, ts
             FROM cloudflare_audit_log
             WHERE account_id = ?1
             ORDER BY ts DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account_id, limit], |r| {
            Ok(CloudflareAuditLog {
                id: r.get(0)?,
                account_id: r.get(1)?,
                action_type: r.get(2)?,
                target_id: r.get(3)?,
                target_name: r.get(4)?,
                summary: r.get(5)?,
                actor: r.get(6)?,
                session_id: r.get(7)?,
                before_state: r.get(8)?,
                after_state: r.get(9)?,
                reverted: r.get::<_, i64>(10)? != 0,
                created_at: r.get(11)?,
                ts: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_cloudflare_audit_log(&self, id: &str) -> Result<Option<CloudflareAuditLog>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, action_type, target_id, target_name, summary, actor, session_id, before_state, after_state, reverted, created_at, ts
             FROM cloudflare_audit_log
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(CloudflareAuditLog {
                id: r.get(0)?,
                account_id: r.get(1)?,
                action_type: r.get(2)?,
                target_id: r.get(3)?,
                target_name: r.get(4)?,
                summary: r.get(5)?,
                actor: r.get(6)?,
                session_id: r.get(7)?,
                before_state: r.get(8)?,
                after_state: r.get(9)?,
                reverted: r.get::<_, i64>(10)? != 0,
                created_at: r.get(11)?,
                ts: r.get(12)?,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    }

    pub fn mark_cloudflare_audit_log_reverted(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE cloudflare_audit_log SET reverted = 1 WHERE id = ?1",
            [id],
        )?;
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

    // ----- Agent plans (present_plan history) -----

    pub fn insert_agent_plan(&self, plan: &AgentPlan) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_plan (id, session_id, workspace_id, title, plan, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                plan = excluded.plan,
                status = excluded.status,
                updated_at = datetime('now')",
            params![
                plan.id,
                plan.session_id,
                plan.workspace_id,
                plan.title,
                plan.plan,
                plan.status,
            ],
        )?;
        Ok(())
    }

    pub fn get_agent_plan(&self, id: &str) -> Result<Option<AgentPlan>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, workspace_id, title, plan, status, created_at, updated_at
             FROM agent_plan WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |r| {
            Ok(AgentPlan {
                id: r.get(0)?,
                session_id: r.get(1)?,
                workspace_id: r.get(2)?,
                title: r.get(3)?,
                plan: r.get(4)?,
                status: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
            })
        })?;
        match rows.next() {
            Some(v) => Ok(Some(v?)),
            None => Ok(None),
        }
    }

    pub fn list_agent_plans(
        &self,
        session_id: Option<&str>,
        workspace_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentPlanMeta>> {
        let conn = self.conn.lock().unwrap();
        let (sql, params_vec): (String, Vec<String>) = match (session_id, workspace_id) {
            (Some(s), Some(w)) => (
                "SELECT id, session_id, workspace_id, title, status, created_at, updated_at
                 FROM agent_plan WHERE session_id = ?1 AND workspace_id = ?2
                 ORDER BY updated_at DESC LIMIT ?3"
                    .into(),
                vec![s.to_string(), w.to_string(), limit.to_string()],
            ),
            (Some(s), None) => (
                "SELECT id, session_id, workspace_id, title, status, created_at, updated_at
                 FROM agent_plan WHERE session_id = ?1
                 ORDER BY updated_at DESC LIMIT ?2"
                    .into(),
                vec![s.to_string(), limit.to_string()],
            ),
            (None, Some(w)) => (
                "SELECT id, session_id, workspace_id, title, status, created_at, updated_at
                 FROM agent_plan WHERE workspace_id = ?1
                 ORDER BY updated_at DESC LIMIT ?2"
                    .into(),
                vec![w.to_string(), limit.to_string()],
            ),
            (None, None) => (
                "SELECT id, session_id, workspace_id, title, status, created_at, updated_at
                 FROM agent_plan ORDER BY updated_at DESC LIMIT ?1"
                    .into(),
                vec![limit.to_string()],
            ),
        };
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec
            .iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |r| {
            Ok(AgentPlanMeta {
                id: r.get(0)?,
                session_id: r.get(1)?,
                workspace_id: r.get(2)?,
                title: r.get(3)?,
                status: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn update_agent_plan_status(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE agent_plan SET status = ?2, updated_at = datetime('now') WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    /// Transition a plan's status only when it is still in `only_if_current`
    /// (e.g. timeout must not overwrite an already-archived/applied plan).
    pub fn update_agent_plan_status_if(
        &self,
        id: &str,
        status: &str,
        only_if_current: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE agent_plan SET status = ?2, updated_at = datetime('now')
             WHERE id = ?1 AND status = ?3",
            params![id, status, only_if_current],
        )?;
        Ok(())
    }

    // ----- Agent file changes (diff history) -----

    /// Insert or replace a file-change record (same id = replace, for revert updates).
    pub fn insert_file_change(&self, rec: &EditRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_file_change
                (id, session_id, workspace_id, scope, vps_id, label, path, before, after, is_new, reverted, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                before = excluded.before,
                after = excluded.after,
                reverted = excluded.reverted",
            params![
                rec.id,
                rec.session_id,
                rec.workspace_id,
                rec.scope,
                rec.vps_id,
                rec.label,
                rec.path,
                rec.before,
                rec.after,
                rec.is_new as i64,
                rec.reverted as i64,
                rec.ts,
            ],
        )?;
        Ok(())
    }

    /// What one agent did over a window, assembled from what was already recorded.
    ///
    /// Three tables answer three halves of "what has it been doing": the tasks it was
    /// given (`goal_sessions.persona_id`), the files it changed while running them
    /// (`agent_file_change`, joined through the `goal:<id>` session id its runs use),
    /// and what it said (`agent_message`). Kept as one query per source rather than one
    /// join, because a task with no edits and an edit outside any task are both normal
    /// and a join would quietly drop one of them.
    /// Define (or redefine) how one of a project's numbers is fetched.
    pub fn upsert_metric_source(
        &self,
        workspace_id: &str,
        name: &str,
        vps_id: &str,
        command: &str,
        unit: Option<&str>,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO metric_source (id, workspace_id, name, vps_id, command, unit, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(workspace_id, name) DO UPDATE SET
               vps_id = excluded.vps_id,
               command = excluded.command,
               unit = COALESCE(excluded.unit, metric_source.unit),
               enabled = excluded.enabled",
            params![
                Uuid::new_v4().to_string(),
                workspace_id,
                name.trim().to_lowercase(),
                vps_id,
                command.trim(),
                unit,
                enabled as i64
            ],
        )?;
        Ok(())
    }

    /// Every way this project knows to fetch a number. `(name, vps_id, command, unit, enabled)`.
    pub fn list_metric_sources(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<(String, String, String, Option<String>, bool)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, vps_id, command, unit, enabled FROM metric_source
             WHERE workspace_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![workspace_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Record one figure for one day, correcting it if that day is already recorded.
    pub fn upsert_metric(
        &self,
        workspace_id: &str,
        name: &str,
        period: &str,
        value: f64,
        unit: Option<&str>,
        note: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO project_metric (id, workspace_id, name, period, value, unit, note, source_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(workspace_id, name, period) DO UPDATE SET
               value = excluded.value,
               unit = COALESCE(excluded.unit, project_metric.unit),
               note = excluded.note,
               source_id = excluded.source_id,
               created_at = datetime('now')",
            params![
                Uuid::new_v4().to_string(),
                workspace_id,
                name.trim().to_lowercase(),
                period,
                value,
                unit,
                note,
                source_id
            ],
        )?;
        Ok(())
    }

    /// Which metrics a project has figures for.
    pub fn metric_names(&self, workspace_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT name FROM project_metric WHERE workspace_id = ?1 ORDER BY name")?;
        let rows = stmt.query_map(params![workspace_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Daily figures for one metric, newest first.
    pub fn metric_series(
        &self,
        workspace_id: &str,
        name: &str,
        since_period: &str,
    ) -> Result<Vec<(String, f64, Option<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT period, value, unit FROM project_metric
             WHERE workspace_id = ?1 AND name = ?2 AND period >= ?3
             ORDER BY period DESC",
        )?;
        let rows = stmt.query_map(params![workspace_id, name.trim().to_lowercase(), since_period], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Total for one metric over a closed period, for comparing one window to the last.
    pub fn metric_total(
        &self,
        workspace_id: &str,
        name: &str,
        from_period: &str,
        to_period: &str,
    ) -> Result<(f64, i64)> {
        let conn = self.conn.lock().unwrap();
        let row = conn.query_row(
            "SELECT COALESCE(SUM(value), 0), COUNT(*) FROM project_metric
             WHERE workspace_id = ?1 AND name = ?2 AND period >= ?3 AND period < ?4",
            params![workspace_id, name.trim().to_lowercase(), from_period, to_period],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(row)
    }

    pub fn agent_tasks_since(
        &self,
        persona_id: &str,
        since_rfc3339: &str,
    ) -> Result<Vec<GoalSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, raw_request, spec_json, status, kanban_json, memory_json,
                    next_check_at, cycles, created_at, updated_at, finished_at, persona_id,
                    workspace_id, outcome, request_id, reported_at, pr_number,
                    approval_state
             FROM goal_sessions
             WHERE persona_id = ?1
               AND COALESCE(finished_at, updated_at, created_at) >= ?2
             ORDER BY COALESCE(finished_at, updated_at, created_at) DESC",
        )?;
        let rows = stmt.query_map(params![persona_id, since_rfc3339], Self::row_to_goal)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Files an agent changed over a window, found through the goal sessions it ran.
    pub fn agent_file_changes_since(
        &self,
        persona_id: &str,
        since_ms: i64,
        limit: i64,
    ) -> Result<Vec<EditRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.session_id, c.workspace_id, c.scope, c.vps_id, c.label, c.path,
                    c.before, c.after, c.is_new, c.reverted, c.ts
             FROM agent_file_change c
             JOIN goal_sessions g ON c.session_id = 'goal:' || g.id
             WHERE g.persona_id = ?1 AND c.ts >= ?2
             ORDER BY c.ts DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![persona_id, since_ms, limit], |r| {
            Ok(EditRecord {
                id: r.get(0)?,
                session_id: r.get(1)?,
                workspace_id: r.get(2)?,
                scope: r.get(3)?,
                vps_id: r.get(4)?,
                label: r.get(5)?,
                path: r.get(6)?,
                before: r.get(7)?,
                after: r.get(8)?,
                is_new: r.get::<_, i64>(9)? != 0,
                reverted: r.get::<_, i64>(10)? != 0,
                ts: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// What an agent said, and what was said to it, over a window.
    pub fn agent_messages_since(
        &self,
        persona_id: &str,
        since_rfc3339: &str,
        limit: i64,
    ) -> Result<Vec<crate::storage::models::AgentMessage>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM agent_message
             WHERE (from_id = ?1 OR to_id = ?1) AND created_at >= ?2
             ORDER BY created_at DESC LIMIT ?3",
            Self::AGENT_MESSAGE_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![persona_id, since_rfc3339, limit],
            Self::row_to_agent_message,
        )?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_file_changes(
        &self,
        session_id: Option<&str>,
        workspace_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<EditRecord>> {
        let conn = self.conn.lock().unwrap();
        let (sql, params_vec): (String, Vec<String>) = match (session_id, workspace_id) {
            (Some(s), Some(w)) => (
                "SELECT id, session_id, workspace_id, scope, vps_id, label, path, before, after, is_new, reverted, ts
                 FROM agent_file_change WHERE session_id = ?1 AND workspace_id = ?2
                 ORDER BY ts DESC LIMIT ?3"
                    .into(),
                vec![s.to_string(), w.to_string(), limit.to_string()],
            ),
            (Some(s), None) => (
                "SELECT id, session_id, workspace_id, scope, vps_id, label, path, before, after, is_new, reverted, ts
                 FROM agent_file_change WHERE session_id = ?1
                 ORDER BY ts DESC LIMIT ?2"
                    .into(),
                vec![s.to_string(), limit.to_string()],
            ),
            (None, Some(w)) => (
                "SELECT id, session_id, workspace_id, scope, vps_id, label, path, before, after, is_new, reverted, ts
                 FROM agent_file_change WHERE workspace_id = ?1
                 ORDER BY ts DESC LIMIT ?2"
                    .into(),
                vec![w.to_string(), limit.to_string()],
            ),
            (None, None) => (
                "SELECT id, session_id, workspace_id, scope, vps_id, label, path, before, after, is_new, reverted, ts
                 FROM agent_file_change ORDER BY ts DESC LIMIT ?1"
                    .into(),
                vec![limit.to_string()],
            ),
        };
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec
            .iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |r| {
            Ok(EditRecord {
                id: r.get(0)?,
                session_id: r.get(1)?,
                workspace_id: r.get(2)?,
                scope: r.get(3)?,
                vps_id: r.get(4)?,
                label: r.get(5)?,
                path: r.get(6)?,
                before: r.get(7)?,
                after: r.get(8)?,
                is_new: r.get::<_, i64>(9)? != 0,
                reverted: r.get::<_, i64>(10)? != 0,
                ts: r.get(11)?,
            })
        })?;
        let mut out: Vec<EditRecord> =
            rows.collect::<std::result::Result<Vec<_>, _>>()?;
        // DESC gives the most recent `limit` rows; reverse to chronological order.
        out.reverse();
        Ok(out)
    }

    pub fn mark_file_change_reverted(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE agent_file_change SET reverted = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_file_changes(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM agent_file_change WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    // ---- Open pull requests (see `crate::ai::pr_guard`) --------------------------

    /// The pull requests `persona_id` has open, oldest first.
    ///
    /// Only rows that are still open: a closed row is history, and history must not
    /// block work. Oldest first because the oldest one is the one that has been idle
    /// longest, and that is the one the agent is being sent back to finish.
    pub fn open_prs_for_persona(&self, persona_id: &str) -> Result<Vec<crate::ai::pr_guard::OpenPr>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT persona_id, workspace_id, branch, pr_number, url, opened_at
             FROM agent_open_pr
             WHERE persona_id = ?1 AND closed_at IS NULL
             ORDER BY opened_at ASC",
        )?;
        let rows = stmt
            .query_map(params![persona_id], Self::row_to_open_pr)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every open pull request across all agents — for a dashboard, or for a sweep that
    /// asks GitHub which of these are actually still open.
    #[allow(dead_code)]
    pub fn all_open_prs(&self) -> Result<Vec<crate::ai::pr_guard::OpenPr>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT persona_id, workspace_id, branch, pr_number, url, opened_at
             FROM agent_open_pr
             WHERE closed_at IS NULL
             ORDER BY opened_at ASC",
        )?;
        let rows = stmt
            .query_map([], Self::row_to_open_pr)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Record that an agent opened a pull request from `branch`.
    ///
    /// Upsert on (persona, workspace, branch), because the same branch being pushed
    /// again is the same pull request. Re-opening a branch that was closed clears
    /// `closed_at` rather than leaving a dead row to shadow the live one.
    pub fn record_open_pr(
        &self,
        persona_id: &str,
        workspace_id: &str,
        branch: &str,
        pr_number: Option<i64>,
        url: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO agent_open_pr (persona_id, workspace_id, branch, pr_number, url)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(persona_id, workspace_id, branch) DO UPDATE SET
                pr_number = COALESCE(excluded.pr_number, agent_open_pr.pr_number),
                url       = COALESCE(excluded.url, agent_open_pr.url),
                opened_at = datetime('now'),
                closed_at = NULL",
            params![persona_id, workspace_id, branch, pr_number, url],
        )?;
        Ok(())
    }

    /// Mark an agent's pull request closed, by number or by branch.
    ///
    /// Returns how many rows it cleared, so a caller can tell "closed it" from "there
    /// was nothing open" instead of reporting success either way.
    pub fn close_open_pr(
        &self,
        persona_id: &str,
        pr_number: Option<i64>,
        branch: Option<&str>,
    ) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = match (pr_number, branch) {
            (Some(n), _) => conn.execute(
                "UPDATE agent_open_pr SET closed_at = datetime('now')
                 WHERE persona_id = ?1 AND pr_number = ?2 AND closed_at IS NULL",
                params![persona_id, n],
            )?,
            (None, Some(b)) => conn.execute(
                "UPDATE agent_open_pr SET closed_at = datetime('now')
                 WHERE persona_id = ?1 AND branch = ?2 AND closed_at IS NULL",
                params![persona_id, b],
            )?,
            // Neither given would close everything, which is never what a caller meant.
            (None, None) => 0,
        };
        Ok(n)
    }

    fn row_to_open_pr(r: &rusqlite::Row) -> rusqlite::Result<crate::ai::pr_guard::OpenPr> {
        Ok(crate::ai::pr_guard::OpenPr {
            persona_id: r.get(0)?,
            workspace_id: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            branch: r.get(2)?,
            pr_number: r.get(3)?,
            url: r.get(4)?,
            opened_at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    }

    // ---- Feature proposals (the approval ladder) --------------------------------

    fn row_to_proposal(r: &rusqlite::Row) -> rusqlite::Result<FeatureProposal> {
        Ok(FeatureProposal {
            id: r.get(0)?,
            workspace_id: r.get(1)?,
            persona_id: r.get(2)?,
            goal_id: r.get(3)?,
            title: r.get(4)?,
            body: r.get(5)?,
            state: r.get(6)?,
            decided_by: r.get(7)?,
            decision_note: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    /// Append-only: `row_to_proposal` reads by position.
    const PROPOSAL_COLS: &'static str =
        "id, workspace_id, persona_id, goal_id, title, body, state, decided_by,
         decision_note, created_at, updated_at";

    pub fn insert_feature_proposal(&self, p: &FeatureProposal) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO feature_proposal
               (id, workspace_id, persona_id, goal_id, title, body, state, decided_by, decision_note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.id,
                p.workspace_id,
                p.persona_id,
                p.goal_id,
                p.title,
                p.body,
                p.state,
                p.decided_by,
                p.decision_note,
            ],
        )?;
        Ok(())
    }

    pub fn get_feature_proposal(&self, id: &str) -> Result<Option<FeatureProposal>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {} FROM feature_proposal WHERE id = ?1", Self::PROPOSAL_COLS);
        match conn.query_row(&sql, [id], Self::row_to_proposal) {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Proposals, newest first. `workspace` and `state` narrow it; either may be None.
    pub fn list_feature_proposals(
        &self,
        workspace: Option<&str>,
        state: Option<&str>,
        limit: i64,
    ) -> Result<Vec<FeatureProposal>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM feature_proposal
             WHERE (?1 IS NULL OR workspace_id = ?1)
               AND (?2 IS NULL OR state = ?2)
             ORDER BY created_at DESC, rowid DESC LIMIT ?3",
            Self::PROPOSAL_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![workspace, state, limit.clamp(1, 500)], Self::row_to_proposal)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Record a decision. Returns false when there is no such proposal.
    pub fn decide_feature_proposal(
        &self,
        id: &str,
        state: &str,
        decided_by: Option<&str>,
        note: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE feature_proposal
                SET state = ?2, decided_by = ?3, decision_note = ?4, updated_at = datetime('now')
              WHERE id = ?1",
            params![id, state, decided_by, note],
        )?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod relock_tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xc-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The point of `relock`: after it, the decrypted database is gone from disk and the
    /// data written before it is still recoverable from the encrypted blob. A lock that
    /// left the plaintext behind would be pure theatre.
    #[test]
    fn relock_removes_the_plaintext_and_keeps_the_data() {
        let dir = scratch("relock");
        let enc = dir.join("xconsole.db.enc");
        let work = dir.join("xconsole.db");
        let key = crate::crypto::new_data_key();

        let db = Db::open_encrypted(&enc, &work, &dir, &key).unwrap();
        db.set_setting("canary", "still-here").unwrap();
        assert!(work.exists(), "the working file is plaintext while unlocked");

        db.relock().unwrap();

        assert!(!work.exists(), "relock must delete the plaintext working file");
        assert!(enc.exists(), "the encrypted blob must remain");
        assert!(db.is_placeholder(), "the live connection must no longer be the real DB");
        // A locked DB answers empty rather than erroring, so the UI can render.
        assert_eq!(db.get_setting("canary").unwrap(), None);

        // Re-open with the same key: the setting written before locking survived.
        let again = Db::open_encrypted(&enc, &work, &dir, &key).unwrap();
        assert_eq!(
            again.get_setting("canary").unwrap().as_deref(),
            Some("still-here")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Re-locking twice, or locking a DB that was never encrypted, must not error — the
    /// idle timer and the Lock button can both fire against an already-locked app.
    #[test]
    fn relock_is_idempotent_and_safe_on_a_plain_db() {
        let dir = scratch("idem");
        let enc = dir.join("xconsole.db.enc");
        let work = dir.join("xconsole.db");
        let key = crate::crypto::new_data_key();

        let db = Db::open_encrypted(&enc, &work, &dir, &key).unwrap();
        db.relock().unwrap();
        db.relock().unwrap();

        assert!(Db::open_locked().unwrap().relock().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod known_host_tests {
    use super::*;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    /// The bug: `known_host` was keyed on (host, port) alone, so a server got exactly one
    /// pinned key. Servers publish several, and which one is presented is decided by
    /// algorithm negotiation — so a change in the client's preference order made a healthy
    /// server look like an impostor, permanently.
    #[test]
    fn each_key_type_is_pinned_separately() {
        let d = db();
        assert!(matches!(
            d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:aaa").unwrap(),
            HostKeyVerdict::PinnedOnFirstUse
        ));
        // Same server, a different one of ITS OWN keys — a first sighting, not an attack.
        assert!(matches!(
            d.verify_host_key("h", 22, "rsa-sha2-512", "SHA256:bbb").unwrap(),
            HostKeyVerdict::PinnedOnFirstUse
        ));
        // ...and both are remembered.
        assert!(matches!(
            d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:aaa").unwrap(),
            HostKeyVerdict::Match
        ));
        assert!(matches!(
            d.verify_host_key("h", 22, "rsa-sha2-512", "SHA256:bbb").unwrap(),
            HostKeyVerdict::Match
        ));
    }

    /// A genuinely different key of the SAME type still has to be refused, and the verdict
    /// has to carry both fingerprints — "pinned X" alone cannot tell a rebuilt server from
    /// an attack, which is the judgement the user is being asked to make.
    #[test]
    fn a_changed_key_is_refused_and_reports_both_fingerprints() {
        let d = db();
        d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:aaa").unwrap();
        match d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:zzz").unwrap() {
            HostKeyVerdict::Mismatch { expected, offered, key_type } => {
                assert_eq!(expected, "SHA256:aaa");
                assert_eq!(offered, "SHA256:zzz");
                assert_eq!(key_type, "ssh-ed25519");
            }
            other => panic!("a changed host key must be refused, got {other:?}"),
        }
    }

    /// Ports are part of the identity: two servers behind one address are two servers.
    #[test]
    fn a_different_port_is_a_different_host() {
        let d = db();
        d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:aaa").unwrap();
        assert!(matches!(
            d.verify_host_key("h", 2222, "ssh-ed25519", "SHA256:ccc").unwrap(),
            HostKeyVerdict::PinnedOnFirstUse
        ));
    }

    /// A no-op autosave must not bump `updated_at` (that write used to keep the
    /// encrypted snapshot rewriter hot at ~10 MB/s).
    #[test]
    fn identical_workspace_upsert_is_a_no_op() {
        let d = db();
        let first = d
            .upsert_workspace(&WorkspaceInput {
                id: Some("ws-1".into()),
                name: "main".into(),
                viewport_json: Some(r#"{"x":0,"y":0,"zoom":1}"#.into()),
                layout_mode: Some("freeform".into()),
                nodes_json: Some("[]".into()),
                color: None,
                icon: None,
                color_mode: None,
                project_json: None,
            })
            .unwrap();
        let t1 = first.updated_at.clone();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = d
            .upsert_workspace(&WorkspaceInput {
                id: Some("ws-1".into()),
                name: "main".into(),
                viewport_json: Some(r#"{"x":0,"y":0,"zoom":1}"#.into()),
                layout_mode: Some("freeform".into()),
                nodes_json: Some("[]".into()),
                color: None,
                icon: None,
                color_mode: None,
                project_json: None,
            })
            .unwrap();
        assert_eq!(second.updated_at, t1);
        let renamed = d
            .upsert_workspace(&WorkspaceInput {
                id: Some("ws-1".into()),
                name: "renamed".into(),
                viewport_json: Some(r#"{"x":0,"y":0,"zoom":1}"#.into()),
                layout_mode: Some("freeform".into()),
                nodes_json: Some("[]".into()),
                color: None,
                icon: None,
                color_mode: None,
                project_json: None,
            })
            .unwrap();
        assert_ne!(renamed.updated_at, t1);
        assert_eq!(renamed.name, "renamed");
    }

    /// Forgetting is what makes a legitimate rebuild recoverable.
    #[test]
    fn forgetting_clears_every_key_type_for_that_host() {
        let d = db();
        d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:aaa").unwrap();
        d.verify_host_key("h", 22, "rsa-sha2-512", "SHA256:bbb").unwrap();
        d.forget_host_key("h", 22).unwrap();
        assert!(matches!(
            d.verify_host_key("h", 22, "ssh-ed25519", "SHA256:new").unwrap(),
            HostKeyVerdict::PinnedOnFirstUse
        ));
        assert!(matches!(
            d.verify_host_key("h", 22, "rsa-sha2-512", "SHA256:new2").unwrap(),
            HostKeyVerdict::PinnedOnFirstUse
        ));
    }
}

#[cfg(test)]
mod channel_read_tests {
    use super::*;
    use crate::storage::models::AgentMessage;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn post(d: &Db, id: &str, from: Option<&str>, channel: &str, mentions: &[&str]) {
        d.insert_agent_message(&AgentMessage {
            id: id.into(),
            from_id: from.map(|s| s.to_string()),
            to_id: None,
            kind: "note".into(),
            body: format!("body {id}"),
            goal_id: None,
            workspace_id: None,
            read_at: None,
            created_at: None,
            channel_id: Some(channel.into()),
            parent_id: None,
            mentions: mentions.iter().map(|s| s.to_string()).collect(),
            resolved_at: None,
        })
        .unwrap();
    }

    fn counts(d: &Db, reader: &str, channel: &str) -> (i64, i64) {
        d.channel_unread_counts(reader)
            .unwrap()
            .into_iter()
            .find(|c| c.channel_id == channel)
            .map(|c| (c.unread, c.mentions))
            .unwrap_or((0, 0))
    }

    /// The whole point of a per-reader cursor: six people in one room each have their
    /// own position, and `agent_message.read_at` (one flag, set by delivery to the
    /// addressee) cannot express that at all.
    #[test]
    fn unread_counts_move_with_the_reader_cursor() {
        let d = db();
        post(&d, "m1", Some("ada"), "company", &[]);
        post(&d, "m2", Some("bruno"), "company", &["ada"]);
        post(&d, "m3", None, "company", &[]);

        // Never opened: everything except your own is unread, and one names you.
        assert_eq!(counts(&d, "ada", "company"), (2, 1));
        // The user wrote m3, so the user sees two and is never mentioned.
        assert_eq!(counts(&d, "", "company"), (2, 0));

        // A cursor before everything changes nothing...
        d.mark_channel_read_at("ada", "company", "1970-01-01 00:00:00")
            .unwrap();
        assert_eq!(counts(&d, "ada", "company"), (2, 1));
        // ...and a cursor after everything clears it, mention badge included.
        d.mark_channel_read_at("ada", "company", "2999-01-01 00:00:00")
            .unwrap();
        assert_eq!(counts(&d, "ada", "company"), (0, 0));
        // One reader moving does not move another.
        assert_eq!(counts(&d, "", "company"), (2, 0));
    }

    /// A mention needle without its quotes would let "ada" match "adamant"; and a room
    /// the reader is not named in must not raise a mention badge.
    #[test]
    fn a_mention_matches_the_whole_id_only() {
        let d = db();
        post(&d, "m1", Some("bruno"), "company", &["adamant"]);
        assert_eq!(counts(&d, "ada", "company"), (1, 0));
    }

    /// Marking read twice must leave one cursor. With reader_id NULLable this silently
    /// inserted a second row every time, and the count never cleared.
    #[test]
    fn marking_a_channel_read_is_idempotent() {
        let d = db();
        post(&d, "m1", Some("ada"), "ws:k8s:general", &[]);
        d.mark_channel_read("", "ws:k8s:general").unwrap();
        d.mark_channel_read("", "ws:k8s:general").unwrap();
        let rows = d.channel_unread_counts("").unwrap();
        assert_eq!(rows.len(), 1, "one row per channel, not one per mark");
        assert_eq!(rows[0].unread, 0);
        assert!(rows[0].last_read_at.is_some());
    }

    /// Threads live in the same table; a room listing that included them would replay
    /// every old correction at the bottom of the room.
    #[test]
    fn a_room_listing_holds_top_level_messages_and_the_thread_holds_the_rest() {
        let d = db();
        post(&d, "root", Some("ada"), "ws:k8s:general", &[]);
        d.insert_agent_message(&AgentMessage {
            id: "reply".into(),
            from_id: None,
            to_id: None,
            kind: "note".into(),
            body: "actually, use 5432".into(),
            goal_id: None,
            workspace_id: None,
            read_at: None,
            created_at: None,
            channel_id: Some("ws:k8s:general".into()),
            parent_id: Some("root".into()),
            mentions: Vec::new(),
            resolved_at: None,
        })
        .unwrap();
        let room = d.list_channel_messages("ws:k8s:general", 50).unwrap();
        assert_eq!(room.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["root"]);
        let thread = d.list_thread("root").unwrap();
        assert_eq!(thread.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["reply"]);
        assert_eq!(thread[0].parent_id.as_deref(), Some("root"));
    }

    /// A log line is a reply target too, so the id it is stored under has to come back.
    #[test]
    fn the_agent_log_round_trips_and_prunes_by_age() {
        let d = db();
        d.insert_agent_log(&AgentLogEntry {
            id: "l1".into(),
            persona_id: "ada".into(),
            workspace_id: Some("k8s".into()),
            goal_id: None,
            session_id: "goal:g1".into(),
            status: "working".into(),
            tool: Some("run_command".into()),
            detail: "kubectl get pods".into(),
            created_at: None,
        })
        .unwrap();
        let log = d.list_agent_log("ada", 50).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].tool.as_deref(), Some("run_command"));
        assert!(log[0].created_at.is_some());
        assert!(d.list_agent_log("bruno", 50).unwrap().is_empty());
        // Written now, so a 7-day window keeps it.
        assert_eq!(d.prune_agent_log(7).unwrap(), 0);
        assert_eq!(d.list_agent_log("ada", 50).unwrap().len(), 1);
    }

    #[test]
    fn the_agent_log_is_trimmed_by_count_per_agent() {
        // Age is the wrong measure on the write path: an agent that ran hard for an
        // hour writes thousands of rows that are all minutes old, and the file is
        // re-encrypted whole when the app locks. A count per agent bounds it where it
        // actually grows — and must not touch anybody else's lines.
        let d = db();
        let line = |persona: &str, n: usize| crate::storage::models::AgentLogEntry {
            id: format!("{persona}-{n}"),
            persona_id: persona.into(),
            workspace_id: None,
            goal_id: None,
            session_id: String::new(),
            status: "working".into(),
            tool: None,
            detail: format!("step {n}"),
            created_at: None,
        };
        for n in 0..10 {
            d.insert_agent_log(&line("ada", n)).unwrap();
        }
        d.insert_agent_log(&line("bruno", 0)).unwrap();

        assert_eq!(d.trim_agent_log("ada", 4).unwrap(), 6);
        let kept = d.list_agent_log("ada", 50).unwrap();
        assert_eq!(kept.len(), 4);
        // The newest survive, and they come back oldest last the way the reader expects.
        assert_eq!(kept.last().unwrap().detail, "step 9");
        assert_eq!(d.list_agent_log("bruno", 50).unwrap().len(), 1);
        // Idempotent: nothing left to drop.
        assert_eq!(d.trim_agent_log("ada", 4).unwrap(), 0);
    }

    #[test]
    fn a_feature_proposal_round_trips_and_records_its_decision() {
        let d = db();
        let p = crate::storage::models::FeatureProposal {
            id: "fp1".into(),
            workspace_id: Some("ws-a".into()),
            persona_id: Some("ada".into()),
            goal_id: Some("g1".into()),
            title: "A billing page".into(),
            body: "why, and what it would cost to keep".into(),
            state: "proposed".into(),
            decided_by: None,
            decision_note: None,
            created_at: None,
            updated_at: None,
        };
        d.insert_feature_proposal(&p).unwrap();

        // Open proposals for one project is the query the settings inbox and the agents'
        // own feature_list both make.
        let open = d.list_feature_proposals(Some("ws-a"), Some("proposed"), 20).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].title, "A billing page");
        assert!(d.list_feature_proposals(Some("ws-b"), None, 20).unwrap().is_empty());

        assert!(d
            .decide_feature_proposal("fp1", "approved", Some("boss"), Some("go on then"))
            .unwrap());
        let after = d.get_feature_proposal("fp1").unwrap().unwrap();
        assert_eq!(after.state, "approved");
        assert_eq!(after.decided_by.as_deref(), Some("boss"));
        assert_eq!(after.decision_note.as_deref(), Some("go on then"));
        // Decided is no longer open, which is what stops the inbox re-asking.
        assert!(d.list_feature_proposals(None, Some("proposed"), 20).unwrap().is_empty());
        // A decision on something that is not there is a false, not a panic.
        assert!(!d.decide_feature_proposal("nope", "approved", None, None).unwrap());
    }
}

#[cfg(test)]
mod open_pr_tests {
    use super::*;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn an_open_pull_request_round_trips_and_is_scoped_to_its_agent() {
        let d = db();
        d.record_open_pr("ada", "k8s", "wip/ada/fix-login", Some(12), Some("https://x/12"))
            .unwrap();
        let rows = d.open_prs_for_persona("ada").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].branch, "wip/ada/fix-login");
        assert_eq!(rows[0].pr_number, Some(12));
        assert!(!rows[0].opened_at.is_empty(), "opened_at is what the refusal quotes");
        // Another agent's PR is not this agent's problem.
        assert!(d.open_prs_for_persona("bruno").unwrap().is_empty());
    }

    #[test]
    fn pushing_the_same_branch_again_updates_the_row_rather_than_adding_one() {
        // Otherwise the second push looks like a second open PR and the agent blocks
        // itself on work it has not finished pushing.
        let d = db();
        d.record_open_pr("ada", "", "wip/ada/dns", None, None).unwrap();
        d.record_open_pr("ada", "", "wip/ada/dns", Some(7), Some("https://x/7"))
            .unwrap();
        let rows = d.open_prs_for_persona("ada").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pr_number, Some(7));
        assert_eq!(rows[0].url.as_deref(), Some("https://x/7"));
    }

    #[test]
    fn closing_a_pull_request_unblocks_the_agent() {
        let d = db();
        d.record_open_pr("ada", "", "wip/ada/dns", Some(7), None).unwrap();
        assert_eq!(d.close_open_pr("ada", Some(7), None).unwrap(), 1);
        assert!(d.open_prs_for_persona("ada").unwrap().is_empty());
        // Closing it twice is not an error, but it is also not a second success.
        assert_eq!(d.close_open_pr("ada", Some(7), None).unwrap(), 0);
    }

    #[test]
    fn a_pull_request_can_be_closed_by_branch_when_its_number_was_never_recorded() {
        let d = db();
        d.record_open_pr("ada", "", "wip/ada/dns", None, None).unwrap();
        assert_eq!(d.close_open_pr("ada", None, Some("wip/ada/dns")).unwrap(), 1);
        // Neither a number nor a branch would mean "close everything", which no caller
        // ever means.
        d.record_open_pr("ada", "", "wip/ada/tls", None, None).unwrap();
        assert_eq!(d.close_open_pr("ada", None, None).unwrap(), 0);
        assert_eq!(d.open_prs_for_persona("ada").unwrap().len(), 1);
    }

    #[test]
    fn every_agents_open_pull_requests_can_be_listed_at_once() {
        let d = db();
        d.record_open_pr("ada", "", "wip/ada/a", Some(1), None).unwrap();
        d.record_open_pr("bruno", "", "wip/bruno/b", Some(2), None).unwrap();
        d.close_open_pr("ada", Some(1), None).unwrap();
        let all = d.all_open_prs().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].persona_id, "bruno");
    }
}

//! At-rest DB encryption (Approach B: working plaintext file + encrypted blob).
//!
//! The app operates on a normal on-disk SQLite file `xconsole.db` (so the existing
//! Mutex<Connection> + every query method are untouched, and the separate MCP process can
//! still share it via WAL). The at-rest artifact is `xconsole.db.enc` — an AES-256-GCM blob.
//! On a write, `commit_hook` flags the DB dirty; a background thread debounces and persists:
//! take a consistent snapshot via SQLite's Online Backup API, integrity-check it, encrypt it,
//! and write it via temp+fsync+atomic-rename so the previous good blob is replaced only at the
//! final instant. Periodic persists skip when `PRAGMA data_version` is unchanged and do not
//! TRUNCATE the WAL (that rewrite was a multi-MB disk hit every 700 ms). A kill mid-persist
//! therefore never destroys the last good ciphertext, and on a clean exit the plaintext
//! working file is removed so only the encrypted blob remains at rest.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use rusqlite::backup::Backup;
use rusqlite::Connection;
use zeroize::Zeroize;

use crate::crypto;

/// Smallest plausible byte size for a real DB image with our schema. Guards against ever
/// overwriting a healthy encrypted DB with a near-empty/corrupt snapshot (e.g. if some other
/// opener created a blank file).
const MIN_DB_BYTES: u64 = 4096;

pub struct PersistCtx {
    pub enc: PathBuf,
    pub work: PathBuf,
    pub data_dir: PathBuf,
    pub key: [u8; crypto::KEY_LEN],
    /// Set by the SQLite commit_hook on every write; cleared by the persister.
    pub dirty: Arc<AtomicBool>,
    /// Set when the lock is disabled, so the daemon persister thread exits.
    pub stopped: Arc<AtomicBool>,
    /// `PRAGMA data_version` of the last snapshot we actually encrypted. Debounced
    /// persists compare against this and no-op when the working DB has not changed,
    /// so a chatty writer cannot rewrite the 4 MB ciphertext every 700 ms.
    pub last_data_version: Mutex<Option<i64>>,
}

impl Drop for PersistCtx {
    /// The data key is the one secret that makes every other secret readable. Wipe it as
    /// soon as the last holder goes away, so re-locking actually removes it from RAM
    /// instead of leaving a copy in a freed allocation for a memory dump to find.
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

/// Verify a SQLite file is structurally sound (used before trusting a recovered plaintext).
pub fn integrity_ok(path: &Path) -> bool {
    let Ok(conn) = Connection::open(path) else {
        return false;
    };
    let res: rusqlite::Result<String> = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0));
    matches!(res, Ok(s) if s == "ok")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("xc-tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Decrypt the at-rest blob into the working plaintext file (atomic). Errors (wrong key or
/// corruption) propagate so the caller can show a locked/restore screen, never a crash.
pub fn decrypt_to_work(enc: &Path, work: &Path, key: &[u8; crypto::KEY_LEN]) -> Result<()> {
    let blob = std::fs::read(enc)?;
    let mut plain = crypto::decrypt(key, &blob).map_err(|e| anyhow!(e))?;
    let r = write_atomic(work, &plain);
    plain.zeroize();
    r
}

fn data_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA data_version", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Take a consistent snapshot of the live DB (Online Backup API — safe against the concurrent
/// MCP writer), integrity-check it, then encrypt it to `enc` via temp+fsync+atomic-rename.
///
/// Used on first-run, unlock, lock, and clean exit. Folds the WAL in first so the working
/// file is a single complete image. The background persister uses [`persist_debounced`]
/// instead — a TRUNCATE checkpoint rewrites the whole DB file and is what showed up as a
/// constant ~10 MB/s disk write when anything kept the dirty flag hot.
pub fn persist_now(conn: &Mutex<Connection>, ctx: &PersistCtx) -> Result<()> {
    persist_inner(conn, ctx, true)
}

/// Background persist: snapshot only when SQLite's data_version moved, and never TRUNCATE
/// the WAL. The working file + WAL are already the crash-recovery truth; `.enc` can lag
/// a few seconds without losing data.
fn persist_debounced(conn: &Mutex<Connection>, ctx: &PersistCtx) -> Result<()> {
    persist_inner(conn, ctx, false)
}

fn persist_inner(conn: &Mutex<Connection>, ctx: &PersistCtx, checkpoint: bool) -> Result<()> {
    if !checkpoint {
        let ver = {
            let src = conn.lock().unwrap();
            data_version(&src)
        };
        if *ctx.last_data_version.lock().unwrap() == Some(ver) {
            return Ok(());
        }
    }

    let snap = ctx.work.with_extension("snap");
    let _ = std::fs::remove_file(&snap);
    let ver;
    {
        let src = conn.lock().unwrap();
        // Only fold WAL on a final flush. Doing it every dirty tick rewrites the whole
        // working file (several MB) even when the commit was a no-op `updated_at` bump.
        if checkpoint {
            let _ = src.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        ver = data_version(&src);
        if !checkpoint && *ctx.last_data_version.lock().unwrap() == Some(ver) {
            return Ok(());
        }
        let mut dst = Connection::open(&snap)?;
        {
            let backup = Backup::new(&src, &mut dst)?;
            backup.run_to_completion(200, Duration::from_millis(0), None)?;
        }
        let ok: String = dst
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap_or_default();
        drop(dst);
        if ok != "ok" {
            let _ = std::fs::remove_file(&snap);
            return Err(anyhow!("snapshot integrity_check failed: {ok}"));
        }
    }
    let mut bytes = std::fs::read(&snap)?;
    let _ = std::fs::remove_file(&snap);

    // GUARD: never replace a healthy existing .enc with a suspiciously small image.
    if (bytes.len() as u64) < MIN_DB_BYTES && ctx.enc.exists() {
        bytes.zeroize();
        return Err(anyhow!(
            "refusing to persist a {}-byte DB image over the existing encrypted DB",
            bytes.len()
        ));
    }

    let blob = crypto::encrypt(&ctx.key, &bytes).map_err(|e| anyhow!(e))?;
    bytes.zeroize();

    let tmp = ctx.enc.with_extension("enc.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&blob)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &ctx.enc)?;
    *ctx.last_data_version.lock().unwrap() = Some(ver);

    if let Some(mut m) = crate::lock::read(&ctx.data_dir) {
        m.generation = m.generation.wrapping_add(1);
        let _ = crate::lock::write(&ctx.data_dir, &m);
    }
    Ok(())
}

/// Background thread: persist whenever the DB is dirty, debounced by a short poll. Daemon
/// thread (dies with the process); the clean-exit hook does the final synchronous persist.
pub fn spawn_persister(conn: Arc<Mutex<Connection>>, ctx: Arc<PersistCtx>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        if ctx.stopped.load(Ordering::Acquire) {
            return;
        }
        if ctx.dirty.swap(false, Ordering::AcqRel) {
            if let Err(e) = persist_debounced(&conn, &ctx) {
                eprintln!("xconsole: encrypted persist failed: {e}");
                ctx.dirty.store(true, Ordering::Release); // retry next tick
            }
        }
    });
}

/// Encrypt a plaintext SQLite file `src` into the at-rest blob `enc` (atomic temp+rename).
/// Used by the one-time migration when the user first enables the lock.
pub fn encrypt_file_to(src: &Path, enc: &Path, key: &[u8; crypto::KEY_LEN]) -> Result<()> {
    let mut bytes = std::fs::read(src)?;
    let blob = crypto::encrypt(key, &bytes).map_err(|e| anyhow!(e))?;
    bytes.zeroize();
    let tmp = enc.with_extension("enc.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&blob)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, enc)?;
    Ok(())
}

/// VERIFY a freshly-written blob is recoverable BEFORE we trust it as the new at-rest truth:
/// decrypt it to a temp file, open it, and run integrity_check. Errors if anything is off, so
/// the migration aborts (leaving the plaintext + lock-off state intact). The temp is removed.
pub fn verify_enc(enc: &Path, key: &[u8; crypto::KEY_LEN]) -> Result<()> {
    let probe = enc.with_extension("verify.tmp");
    decrypt_to_work(enc, &probe, key)?;
    let ok = integrity_ok(&probe);
    let _ = std::fs::remove_file(&probe);
    if ok {
        Ok(())
    } else {
        Err(anyhow!("encrypted DB failed verification (integrity_check)"))
    }
}

/// Best-effort removal of the plaintext working files after a clean-exit persist, so only the
/// encrypted blob remains at rest. (Fails harmlessly if another process holds the file open.)
pub fn cleanup_work_files(work: &Path) {
    let base = work.display().to_string();
    // "" / -wal / -shm are the live plaintext SQLite files. ".premigrate.bak" is a stale
    // plaintext rollback copy from enabling the lock — reaping it here is a backstop so any
    // copy left behind by an older build (before setup_lock deleted it) can't linger as an
    // unencrypted snapshot once the DB is encrypted.
    for suffix in ["", "-wal", "-shm", ".premigrate.bak"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{base}{suffix}")));
    }
}

#[cfg(test)]
mod persist_skip_tests {
    use super::*;

    #[test]
    fn debounced_persist_skips_when_data_version_unchanged() {
        let dir = std::env::temp_dir().join(format!(
            "xc-pskip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let work = dir.join("xconsole.db");
        let enc = dir.join("xconsole.db.enc");
        let raw = Connection::open(&work).unwrap();
        raw.execute_batch("CREATE TABLE t(x BLOB);").unwrap();
        raw.execute("INSERT INTO t VALUES (?1)", [vec![0u8; 5000]])
            .unwrap();
        drop(raw);
        let conn = Mutex::new(Connection::open(&work).unwrap());
        let ctx = PersistCtx {
            enc: enc.clone(),
            work,
            data_dir: dir.clone(),
            key: crate::crypto::new_data_key(),
            dirty: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            last_data_version: Mutex::new(None),
        };
        persist_now(&conn, &ctx).unwrap();
        let t1 = std::fs::metadata(&enc).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        persist_debounced(&conn, &ctx).unwrap();
        persist_debounced(&conn, &ctx).unwrap();
        let t2 = std::fs::metadata(&enc).unwrap().modified().unwrap();
        assert_eq!(t1, t2, "idle persist must not rewrite the ciphertext");
        {
            let c = conn.lock().unwrap();
            c.execute("INSERT INTO t VALUES (?1)", [vec![1u8; 16]])
                .unwrap();
        }
        persist_debounced(&conn, &ctx).unwrap();
        let t3 = std::fs::metadata(&enc).unwrap().modified().unwrap();
        assert_ne!(t3, t1, "a real write must still refresh the ciphertext");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

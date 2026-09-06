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
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use rusqlite::backup::Backup;
use rusqlite::Connection;
use zeroize::Zeroize;

use crate::crypto;

/// Smallest plausible byte size for a real DB image with our schema. Guards against ever
/// overwriting a healthy encrypted DB with a near-empty/corrupt snapshot (e.g. if some other
/// opener created a blank file).
const MIN_DB_BYTES: u64 = 4096;

/// How often the background persister wakes to look at the dirty flag (an atomic load).
const POLL: Duration = Duration::from_millis(1000);
/// How long the DB must be quiet before a background persist runs, so one agent turn
/// writing a hundred rows costs one snapshot instead of a hundred.
const QUIET: Duration = Duration::from_secs(3);
/// Floor on the gap between two background persists. Snapshot+encrypt cost is proportional
/// to the WHOLE database, so without a floor a chatty writer turns a 77 MB DB into a
/// permanent ~150 MB/s of disk traffic. `.enc` lagging by up to a minute loses nothing:
/// the working file + WAL are the crash-recovery truth, and a clean exit flushes anyway.
const MIN_INTERVAL: Duration = Duration::from_secs(60);

pub struct PersistCtx {
    pub enc: PathBuf,
    pub work: PathBuf,
    pub data_dir: PathBuf,
    pub key: [u8; crypto::KEY_LEN],
    /// Set by the SQLite commit_hook on every write; cleared by the persister.
    pub dirty: Arc<AtomicBool>,
    /// Set when the lock is disabled, so the daemon persister thread exits.
    pub stopped: Arc<AtomicBool>,
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

fn data_version(conn: &Connection) -> (i64, i64) {
    let dv: i64 = conn
        .query_row("PRAGMA data_version", [], |r| r.get(0))
        .unwrap_or(0);
    let tc = conn.total_changes() as i64;
    (dv, tc)
}

/// Take a consistent snapshot of the live DB, integrity-check it, then encrypt it to `enc`
/// via temp+fsync+atomic-rename. Used on first-run, unlock, lock, and clean exit — the
/// points where `.enc` must be exactly current. Folds the WAL in first so the working file
/// is a single complete image, and takes the shared connection because the app is either
/// starting up or shutting down, so a pause is free.
///
/// The background persister deliberately does NOT come through here — see
/// [`spawn_persister`] for why it uses its own connection and a much slower cadence.
pub fn persist_now(conn: &Mutex<Connection>, ctx: &PersistCtx) -> Result<()> {
    let src = conn.lock().unwrap();
    let _ = src.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    snapshot_and_encrypt(&src, ctx)?;
    Ok(())
}

/// Snapshot `src` → integrity-check → encrypt → atomically replace `enc`. Returns the
/// data-version of the image that was persisted, so the caller can skip re-persisting an
/// unchanged DB. Takes a plain `&Connection` so the caller decides which one to use.
fn snapshot_and_encrypt(src: &Connection, ctx: &PersistCtx) -> Result<(i64, i64)> {
    let snap = ctx.work.with_extension("snap");
    let _ = std::fs::remove_file(&snap);
    let ver = data_version(src);
    {
        let mut dst = Connection::open(&snap)?;
        {
            let backup = Backup::new(src, &mut dst)?;
            // A pause of zero busy-spins a whole core whenever the source is locked by
            // another connection, which on a shared WAL database is not rare. One
            // millisecond a step costs about a tenth of a second over a large database
            // and turns that spin into a wait.
            backup.run_to_completion(200, Duration::from_millis(1), None)?;
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
    // The snapshot is a full PLAINTEXT copy of the database. Remove it as soon as it is in
    // memory — leaving it around defeats the point of encrypting at rest, and a crash
    // mid-persist used to strand a full-size plaintext image next to the ciphertext.
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

    if let Some(mut m) = crate::lock::read(&ctx.data_dir) {
        m.generation = m.generation.wrapping_add(1);
        let _ = crate::lock::write(&ctx.data_dir, &m);
    }
    Ok(ver)
}

/// Remove a plaintext snapshot stranded by a crash (or a kill) mid-persist. Called at
/// startup: a full-size plaintext copy of the database sitting next to the ciphertext is
/// both a disk hog and exactly the thing at-rest encryption is supposed to prevent.
pub fn cleanup_stale_snapshot(work: &Path) {
    let snap = work.with_extension("snap");
    let _ = std::fs::remove_file(&snap);
    let _ = std::fs::remove_file(snap.with_extension("snap-journal"));
}

/// How much newer than the encrypted blob the plaintext working file may be before we stop
/// believing a clean-exit marker. A real clean exit writes the blob and the marker seconds
/// apart, then SQLite touches the working file once more as it closes — so a small gap is
/// normal. A gap of *minutes* means the blob is not actually current and the marker is
/// lying, which is the difference between tidying up and destroying the user's data.
const CLEAN_MARKER_SLACK: Duration = Duration::from_secs(600);

/// Decide what to do with the plaintext working file at startup.
///
/// A clean exit leaves a `.clean` marker meaning "the encrypted blob is current, so the
/// plaintext is disposable" — Windows cannot delete the still-open working file at exit, so
/// the next launch does it. Trusting that marker blindly is dangerous: a marker written
/// after a *failed* final persist points at a stale blob, and deleting the plaintext then
/// throws away the only current copy of the database. So verify the blob really is as new
/// as the working file, and keep the plaintext whenever that is in any doubt — an extra
/// plaintext file is a problem you can fix later, deleted data is not.
pub fn discard_plaintext_if_blob_is_current(enc: &Path, work: &Path) {
    let marker = enc.with_extension("clean");
    let had_clean_exit = marker.exists();
    let _ = std::fs::remove_file(&marker);
    cleanup_stale_snapshot(work);
    if !had_clean_exit || !work.exists() {
        return;
    }
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (mtime(enc), mtime(work)) {
        (Some(e), Some(w)) => {
            let lag = w.duration_since(e).unwrap_or_default();
            if lag <= CLEAN_MARKER_SLACK {
                cleanup_work_files(work);
            } else {
                crate::diag(&format!(
                    "clean-exit marker found, but the encrypted DB is {}s older than the                      plaintext working file — keeping the plaintext, which is the newer copy",
                    lag.as_secs()
                ));
            }
        }
        _ => crate::diag(
            "clean-exit marker found but the encrypted DB is unreadable — keeping the plaintext",
        ),
    }
}

/// Background thread: keep `.enc` reasonably current without making the app unusable.
///
/// Two things here are load-bearing, and both were learned the hard way:
///
/// 1. **Its own connection.** A snapshot reads the entire database. Taking it through the
///    shared `Mutex<Connection>` held that lock for a second or more at a time, and every
///    Tauri command that touches the DB blocks on the same mutex — so while an agent was
///    writing, opening another view froze the app. WAL lets a second connection read a
///    consistent image concurrently, so the persister opens its own (`query_only`, so it
///    cannot write) and never takes the lock the UI needs.
///
/// 2. **A rate floor, not a dirty flag.** Persisting on every dirty tick means re-encrypting
///    the WHOLE database — a 77 MB DB became a sustained ~20 MB/s write + ~31 MB/s read for
///    as long as anything kept writing. Waiting for the DB to go quiet and then persisting
///    at most once a minute costs the same per-persist but bounds the damage.
///
/// Daemon thread (dies with the process); the clean-exit hook does the final synchronous
/// persist through [`persist_now`].
pub fn spawn_persister(conn: Arc<Mutex<Connection>>, ctx: Arc<PersistCtx>) {
    std::thread::spawn(move || {
        let open_private = |work: &Path| -> Option<Connection> {
            let c = Connection::open(work).ok()?;
            c.pragma_update(None, "query_only", "ON").ok()?;
            Some(c)
        };
        let mut private = open_private(&ctx.work);
        let mut dirty_since: Option<Instant> = None;
        let mut last_attempt = Instant::now();
        // Kept here rather than shared: `data_version` and `total_changes` are per-connection
        // readings, so a value taken on this thread's connection is only ever meaningful
        // against another value from the same one.
        let mut last_persisted: Option<(i64, i64)> = None;

        loop {
            std::thread::sleep(POLL);
            if ctx.stopped.load(Ordering::Acquire) {
                return;
            }
            if ctx.dirty.swap(false, Ordering::AcqRel) {
                dirty_since.get_or_insert_with(Instant::now);
            }
            let Some(since) = dirty_since else { continue };
            if since.elapsed() < QUIET || last_attempt.elapsed() < MIN_INTERVAL {
                continue;
            }
            if private.is_none() {
                private = open_private(&ctx.work);
            }

            last_attempt = Instant::now();
            let outcome = match private.as_ref() {
                Some(c) => {
                    if last_persisted == Some(data_version(c)) {
                        dirty_since = None;
                        continue;
                    }
                    snapshot_and_encrypt(c, &ctx)
                }
                // Private connection unavailable (rare): fall back to the shared one rather
                // than silently stop persisting. Slower for the UI, but correct.
                None => {
                    let src = conn.lock().unwrap();
                    snapshot_and_encrypt(&src, &ctx)
                }
            };

            match outcome {
                Ok(ver) => {
                    last_persisted = Some(ver);
                    dirty_since = None;
                }
                Err(e) => {
                    // This used to be an `eprintln!`. Release builds set
                    // `windows_subsystem = "windows"`, so stderr goes nowhere — which is how
                    // `.enc` sat a month stale while the retry loop rewrote the whole
                    // database every second and nobody could see why.
                    crate::diag(&format!("encrypted persist failed: {e}"));
                    // Reopen next round in case the connection itself went bad, and let
                    // MIN_INTERVAL back the retry off instead of hammering the disk.
                    private = None;
                }
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

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xc-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seeded(dir: &Path) -> (PathBuf, PathBuf) {
        let work = dir.join("xconsole.db");
        let enc = dir.join("xconsole.db.enc");
        let raw = Connection::open(&work).unwrap();
        raw.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x BLOB);")
            .unwrap();
        raw.execute("INSERT INTO t VALUES (?1)", [vec![0u8; 5000]])
            .unwrap();
        drop(raw);
        (work, enc)
    }

    fn ctx_for(dir: &Path, work: &Path, enc: &Path) -> PersistCtx {
        PersistCtx {
            enc: enc.to_path_buf(),
            work: work.to_path_buf(),
            data_dir: dir.to_path_buf(),
            key: crate::crypto::new_data_key(),
            dirty: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The persister only re-encrypts when the DB actually moved. Without this an idle app
    /// rewrites the entire ciphertext on every tick.
    #[test]
    fn an_idle_db_reports_an_unchanged_version_so_the_persister_skips() {
        let dir = scratch("pskip");
        let (work, enc) = seeded(&dir);
        let conn = Mutex::new(Connection::open(&work).unwrap());
        let ctx = ctx_for(&dir, &work, &enc);

        let probe = Connection::open(&work).unwrap();
        probe.pragma_update(None, "query_only", "ON").unwrap();
        persist_now(&conn, &ctx).unwrap();
        // Nothing wrote afterwards, so the version the persister observes on its own
        // connection is unchanged — which is exactly the condition it uses to do nothing.
        let persisted = Some(data_version(&probe));

        {
            let c = conn.lock().unwrap();
            c.execute("INSERT INTO t VALUES (?1)", [vec![1u8; 16]]).unwrap();
        }
        assert_ne!(
            Some(data_version(&probe)),
            persisted,
            "a real write must be visible to the persister's own connection"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The reason the UI used to freeze: the snapshot ran through the one `Mutex<Connection>`
    /// every Tauri command needs, and holds it for as long as it takes to copy the whole
    /// database. Snapshotting must work while that mutex is held by someone else.
    #[test]
    fn a_snapshot_runs_while_the_shared_connection_is_locked() {
        let dir = scratch("pnolock");
        let (work, enc) = seeded(&dir);
        let shared = Mutex::new(Connection::open(&work).unwrap());
        let ctx = ctx_for(&dir, &work, &enc);

        // Simulate a command holding the shared connection for the whole persist.
        let held = shared.lock().unwrap();

        let private = Connection::open(&work).unwrap();
        private.pragma_update(None, "query_only", "ON").unwrap();
        snapshot_and_encrypt(&private, &ctx).expect("snapshot must not need the shared lock");

        assert!(enc.exists(), "ciphertext must have been written");
        verify_enc(&enc, &ctx.key).expect("what we wrote must decrypt and pass integrity");
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `query_only` connection is the persister's guarantee that it can never corrupt the
    /// database it is reading.
    #[test]
    fn the_persisters_connection_cannot_write() {
        let dir = scratch("pro");
        let (work, _enc) = seeded(&dir);
        let private = Connection::open(&work).unwrap();
        private.pragma_update(None, "query_only", "ON").unwrap();
        assert!(private
            .execute("INSERT INTO t VALUES (?1)", [vec![2u8; 8]])
            .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The plaintext snapshot is a full copy of the database. Leaving it on disk defeats
    /// at-rest encryption outright, so a successful persist must not strand one.
    #[test]
    fn a_persist_leaves_no_plaintext_snapshot_behind() {
        let dir = scratch("psnap");
        let (work, enc) = seeded(&dir);
        let conn = Mutex::new(Connection::open(&work).unwrap());
        let ctx = ctx_for(&dir, &work, &enc);
        persist_now(&conn, &ctx).unwrap();
        assert!(
            !work.with_extension("snap").exists(),
            "the plaintext snapshot must be removed once it is encrypted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

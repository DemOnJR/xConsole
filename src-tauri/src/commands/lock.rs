//! App-lock commands: master-password setup/unlock, change-password, forget-device, the
//! disable path, and the unencrypted-backup escape hatch. The encryption ENGINE is in
//! `storage/encrypt.rs`; these wire it to the user. No recovery by design — a forgotten
//! password with no remembered device means the data is unrecoverable.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroize;

use crate::crypto;
use crate::lock;
use crate::secrets;
use crate::storage::{encrypt, Db};

/// Minimum master-password length. The entire confidentiality of a stolen encrypted DB rests
/// on this password (the algorithm, salt, and iteration count are all public in this
/// open-source build), so a short password is the practical weakest link — keep it long.
const MIN_PASSWORD_LEN: usize = 12;

/// The in-RAM data key while unlocked (None when locked). Managed Tauri state.
#[derive(Default)]
pub struct DataKey(pub Mutex<Option<[u8; crypto::KEY_LEN]>>);

#[derive(Serialize)]
pub struct LockStatus {
    /// A lock is configured for this install.
    pub enabled: bool,
    /// The DB is currently unlocked (key in RAM).
    pub unlocked: bool,
    /// This device has the key remembered (keychain) — silent unlock at launch.
    pub remembered: bool,
    /// Saved credentials (SSH passwords/keys, API tokens) are stored in the OS keychain
    /// encrypted with the data key, so reading the credential store yields ciphertext.
    /// False when no lock is configured — there is then no key to encrypt them with.
    pub secrets_encrypted: bool,
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
fn enc_path(dir: &Path) -> PathBuf {
    dir.join("xconsole.db.enc")
}
fn work_path(dir: &Path) -> PathBuf {
    dir.join("xconsole.db")
}

#[tauri::command]
pub fn lock_status(
    app: AppHandle,
    db: State<Db>,
    datakey: State<DataKey>,
) -> Result<LockStatus, String> {
    let dir = data_dir(&app)?;
    Ok(LockStatus {
        enabled: lock::is_lock_enabled(&dir),
        unlocked: datakey.0.lock().unwrap().is_some(),
        remembered: secrets::get_data_key().ok().flatten().is_some(),
        secrets_encrypted: secrets::encryption_opted_in(&db) && secrets::wrapping_active(),
    })
}

/// First-time enable: encrypt the existing plaintext DB with verify-before-commit, write the
/// manifest, optionally remember the key, and flip the running DB to encrypted IN PLACE.
#[tauri::command]
pub fn setup_lock(
    app: AppHandle,
    db: State<Db>,
    datakey: State<DataKey>,
    mut password: String,
    remember: bool,
) -> Result<(), String> {
    let dir = data_dir(&app)?;
    if lock::is_lock_enabled(&dir) {
        return Err("App lock is already enabled.".into());
    }
    if password.trim().len() < MIN_PASSWORD_LEN {
        password.zeroize();
        return Err(format!(
            "Please choose a master password of at least {MIN_PASSWORD_LEN} characters."
        ));
    }
    let enc = enc_path(&dir);
    let work = work_path(&dir);
    let data_key = crypto::new_data_key();

    // 1) Pre-migration safety copy of the plaintext DB. This is a TEMPORARY rollback copy
    //    used only until the encrypted blob is verified + live; it is deleted in step 5.
    //    (It must never outlive the migration — a lingering plaintext copy would defeat the
    //    whole point of at-rest encryption for a stolen/synced data dir.)
    let premigrate = dir.join("xconsole.db.premigrate.bak");
    db.backup_to(&premigrate)
        .map_err(|e| format!("pre-migration backup failed: {e}"))?;

    // 2) Consistent snapshot → encrypt → .enc.
    let snap = dir.join("xconsole.db.migsnap");
    let make = db
        .backup_to(&snap)
        .map_err(|e| format!("snapshot failed: {e}"))
        .and_then(|_| encrypt::encrypt_file_to(&snap, &enc, &data_key).map_err(|e| format!("encrypt failed: {e}")));
    let _ = std::fs::remove_file(&snap);
    make?;

    // 3) VERIFY the blob decrypts + passes integrity_check BEFORE we commit the switch.
    if let Err(e) = encrypt::verify_enc(&enc, &data_key) {
        let _ = std::fs::remove_file(&enc);
        return Err(format!("verification failed — lock NOT enabled, your data is untouched: {e}"));
    }

    // 4) Commit: manifest, remember (optional), flip the running DB to encrypted.
    let manifest = lock::build_manifest(&password, &data_key, 1)?;
    lock::write(&dir, &manifest).map_err(|e| format!("write manifest: {e}"))?;
    if remember {
        secrets::set_data_key(&data_key).map_err(|e| format!("keychain: {e}"))?;
    }
    db.enable_encryption_in_place(&enc, &work, &dir, &data_key)
        .map_err(|e| e.to_string())?;

    // 4b) Encrypt the keychain entries now that a data key exists. This is on unless the
    //     user has explicitly turned it off: without it the keychain still hands out
    //     directly usable passwords to anything running as this user, which would make
    //     the lock protect the database and nothing else.
    if secrets::encryption_opted_in(&db) {
        secrets::rekey_all(&secrets::all_secret_keys(&db), Some(data_key));
    }

    // 5) The encrypted DB is verified and live, so the plaintext rollback copy has done its
    //    job. Delete it now — otherwise a full unencrypted snapshot of all chats/workspaces/
    //    memory would sit on disk forever next to the encrypted blob, silently defeating the
    //    lock for anyone whose data dir is later copied/synced/stolen.
    let _ = std::fs::remove_file(&premigrate);

    *datakey.0.lock().unwrap() = Some(data_key);
    password.zeroize();
    Ok(())
}

/// Unlock a locked placeholder DB with the master password (swaps the real connection in).
#[tauri::command]
pub fn unlock_with_password(
    app: AppHandle,
    db: State<Db>,
    datakey: State<DataKey>,
    idle: State<AutoLock>,
    mut password: String,
    remember: bool,
) -> Result<(), String> {
    // Unlocking IS activity, and the idle clock has been running the whole time the app sat
    // locked. Without this the very next watcher tick sees hours of idleness and locks again
    // within 20 seconds — and it would keep doing that forever if the frontend heartbeat ever
    // failed, since nothing else resets the clock. The timeout must be measured from here.
    idle.touch();
    let dir = data_dir(&app)?;
    let manifest = lock::read(&dir).ok_or("App lock isn't configured.")?;
    let key = lock::unlock(&manifest, &password)
        .map_err(|_| "Wrong password — there is no reset.".to_string())?;
    password.zeroize();

    db.unlock_into(&enc_path(&dir), &work_path(&dir), &dir, &key)
        .map_err(|e| format!("unlock failed: {e}"))?;
    if remember {
        secrets::set_data_key(&key).map_err(|e| format!("keychain: {e}"))?;
    }
    // Secrets are encrypted with the data key, so they only become readable now. The
    // re-key also catches up anything still stored in the clear — secrets written by a
    // build from before wrapping existed, for instance.
    secrets::set_wrapping_key(Some(key));
    if secrets::encryption_opted_in(&db) {
        secrets::rekey_all(&secrets::all_secret_keys(&db), Some(key));
    }
    *datakey.0.lock().unwrap() = Some(key);
    Ok(())
}

/// Re-wrap the same data key under a new password (cheap — no DB re-encrypt).
#[tauri::command]
pub fn change_password(
    app: AppHandle,
    mut old_password: String,
    mut new_password: String,
) -> Result<(), String> {
    let dir = data_dir(&app)?;
    let manifest = lock::read(&dir).ok_or("App lock isn't configured.")?;
    let key = lock::unlock(&manifest, &old_password)
        .map_err(|_| "Current password is incorrect.".to_string())?;
    old_password.zeroize();
    if new_password.trim().len() < MIN_PASSWORD_LEN {
        new_password.zeroize();
        return Err(format!(
            "Please choose a master password of at least {MIN_PASSWORD_LEN} characters."
        ));
    }
    let new_manifest = lock::build_manifest(&new_password, &key, manifest.generation)?;
    new_password.zeroize();
    lock::write(&dir, &new_manifest).map_err(|e| format!("write manifest: {e}"))
}

/// Turn credential encryption on or off, converting every stored secret to match.
///
/// Separate from the app lock on purpose. The lock protects the database; this protects
/// the OS credential store, and unlike the lock it is **forward-only for older builds**:
/// they don't recognise a wrapped value and will send the ciphertext as the password,
/// failing every login. Making it an explicit, reversible switch means a user can roll
/// back to an older xConsole without their credentials becoming unreadable — and means
/// it can never happen as a silent side effect of launching a newer build once.
#[tauri::command]
pub fn set_secret_encryption(
    db: State<Db>,
    datakey: State<DataKey>,
    enabled: bool,
) -> Result<usize, String> {
    let key = *datakey
        .0
        .lock()
        .unwrap();
    let Some(key) = key else {
        return Err(
            "Unlock xConsole with your master password first — the encryption key isn't loaded."
                .into(),
        );
    };

    // Install the key before converting: reading an already-wrapped secret needs it,
    // whichever direction we're going.
    secrets::set_wrapping_key(Some(key));
    let changed = secrets::rekey_all(
        &secrets::all_secret_keys(&db),
        if enabled { Some(key) } else { None },
    );
    // Leave the key installed when on (so later writes wrap); clear it when off.
    secrets::set_wrapping_key(if enabled { Some(key) } else { None });

    db.set_setting(
        secrets::ENCRYPT_SECRETS_SETTING,
        if enabled { "true" } else { "false" },
    )
    .map_err(|e| e.to_string())?;
    Ok(changed)
}

/// Settings key for the idle auto-lock timeout, in minutes. `0` disables it.
pub const AUTO_LOCK_SETTING: &str = "security.auto_lock_minutes";
/// Auto-lock timeout used when the user has never chosen one.
///
/// An hour, not the 15 minutes this shipped with. Reading logs, watching a build or working
/// in another window are all "idle" by the input-only definition, so a short default locks
/// people out mid-task — and a security feature that interrupts you is one you turn off.
pub const AUTO_LOCK_DEFAULT_MINUTES: u64 = 60;
/// Upper bound we will honour. A timeout measured in days is indistinguishable from "off"
/// but *looks* like protection, which is worse than being told it is off.
const AUTO_LOCK_MAX_MINUTES: u64 = 8 * 60;

/// How long the app has been idle. Fed by [`note_activity`] from real user input.
pub struct AutoLock {
    last_activity: Mutex<std::time::Instant>,
    /// Has the UI ever reported input? Only [`note_activity`] sets this.
    heartbeat_seen: std::sync::atomic::AtomicBool,
}

impl Default for AutoLock {
    fn default() -> Self {
        Self {
            last_activity: Mutex::new(std::time::Instant::now()),
            heartbeat_seen: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl AutoLock {
    /// Reset the idle clock. Called on unlock as well as on input, so the timeout is
    /// always measured from the last thing that was definitely the user.
    pub fn touch(&self) {
        *self.last_activity.lock().unwrap() = std::time::Instant::now();
    }

    /// Reset the clock *and* confirm the UI's heartbeat is working.
    fn beat(&self) {
        self.touch();
        self.heartbeat_seen
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn idle_for(&self) -> std::time::Duration {
        self.last_activity.lock().unwrap().elapsed()
    }

    /// Whether idleness can be measured at all.
    ///
    /// If the UI has never reported input, we are not seeing an idle user — we are blind.
    /// Auto-locking on that would fire on a fixed schedule *while the app is in use*, which
    /// is indistinguishable from the app being broken and is exactly how a security feature
    /// gets switched off for good. So this fails open, and [`lock_now`] still works.
    pub fn can_measure_idleness(&self) -> bool {
        self.heartbeat_seen
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Is this device remembered — i.e. is the data key sitting in the OS keychain?
pub fn device_remembered() -> bool {
    secrets::get_data_key().ok().flatten().is_some()
}

/// Configured idle timeout, clamped. `None` means auto-lock is off.
pub fn auto_lock_timeout(db: &Db) -> Option<std::time::Duration> {
    auto_lock_timeout_for(db, device_remembered())
}

/// The timeout decision, with the keychain lookup passed in so it can be tested.
///
/// **A remembered device does not idle-lock unless the user asks it to.** "Remember on
/// this device" puts the data key in the OS keychain and the app unlocks itself silently
/// at launch — so anyone sitting at the unattended machine defeats an idle lock by closing
/// the window and opening it again. It buys close to nothing there, and it costs every
/// live SSH, SFTP and database session, because locking closes them all. Being thrown out
/// of a long-running remote job by a lock that a relaunch would have walked straight
/// through is the wrong trade.
///
/// It stays on by default when the device is *not* remembered, which is the case where
/// locking genuinely means something: the key is not on the machine, so the lock screen is
/// a real barrier. Either way an explicit choice always wins — including choosing a
/// timeout while remembered.
pub fn auto_lock_timeout_for(db: &Db, remembered: bool) -> Option<std::time::Duration> {
    let minutes = match db.get_setting(AUTO_LOCK_SETTING) {
        Ok(Some(v)) => v.parse::<u64>().unwrap_or(AUTO_LOCK_DEFAULT_MINUTES),
        _ if remembered => return None,
        // Unset and not remembered: default ON. An idle unlocked app is the realistic way
        // credentials get used by someone who is not the user.
        _ => AUTO_LOCK_DEFAULT_MINUTES,
    };
    if minutes == 0 {
        return None;
    }
    Some(std::time::Duration::from_secs(
        minutes.clamp(1, AUTO_LOCK_MAX_MINUTES) * 60,
    ))
}

/// Record user activity, resetting the idle timer. Called from the UI on real input.
#[tauri::command]
pub fn note_activity(idle: State<AutoLock>) {
    idle.beat();
}

#[tauri::command]
pub fn get_auto_lock_minutes(db: State<Db>) -> u64 {
    // Report what will actually happen, not the raw setting: with no explicit choice on a
    // remembered device the answer is "never", and the dropdown has to say so.
    auto_lock_timeout(&db)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

#[tauri::command]
pub fn set_auto_lock_minutes(db: State<Db>, minutes: u64) -> Result<(), String> {
    let minutes = if minutes == 0 {
        0
    } else {
        minutes.clamp(1, AUTO_LOCK_MAX_MINUTES)
    };
    db.set_setting(AUTO_LOCK_SETTING, &minutes.to_string())
        .map_err(|e| e.to_string())
}

/// Close every kind of standing remote access the app holds open.
///
/// All three are the same thing from a security point of view: an authenticated channel to
/// somebody's server, opened with credentials the lock is supposed to protect. Closing only
/// the terminals would leave a "locked" app that can still browse files and read databases.
pub fn close_everything(
    sessions: &crate::ssh::SessionManager,
    sftp: &crate::ssh::SftpManager,
    db_sessions: &crate::commands::db::DbSessions,
) -> usize {
    sessions.disconnect_all() + sftp.disconnect_all() + db_sessions.disconnect_all()
}

/// Lock the app **without quitting**: close every live shell, flush and re-encrypt the
/// database, delete the plaintext working file, and forget the data key.
///
/// This is the counterpart the app was missing. Without it the only way to stop an unlocked
/// xConsole from being usable was to quit it, so an unattended desktop meant live root
/// sessions plus a decrypted database on disk for as long as the app stayed open.
///
/// Order matters: sessions first (they are the live capability), then the database (so the
/// plaintext file goes away), then the keys. Every step is attempted even if an earlier one
/// reports a problem — a lock that gives up halfway is worse than a noisy one.
#[tauri::command]
pub fn lock_now(
    app: AppHandle,
    db: State<Db>,
    datakey: State<DataKey>,
    sessions: State<crate::ssh::SessionManager>,
    sftp: State<crate::ssh::SftpManager>,
    db_sessions: State<crate::commands::db::DbSessions>,
) -> Result<u64, String> {
    let dir = data_dir(&app)?;
    if !lock::is_lock_enabled(&dir) {
        return Err(
            "Set a master password first — without one there is nothing to lock the app with."
                .into(),
        );
    }

    let closed = close_everything(&sessions, &sftp, &db_sessions);
    let relocked = db.relock();

    // Forget the keys regardless of how the DB fared: leaving them in RAM after the user
    // asked to lock is the one outcome with no upside.
    secrets::set_wrapping_key(None);
    if let Some(mut key) = datakey.0.lock().unwrap().take() {
        key.zeroize();
    }

    let _ = app.emit("app://locked", closed as u64);
    relocked.map_err(|e| format!("locked, but saving the database failed: {e}"))?;
    Ok(closed as u64)
}

/// Forget the key on this device — next launch will require the master password.
#[tauri::command]
pub fn forget_device() -> Result<(), String> {
    secrets::clear_data_key().map_err(|e| e.to_string())
}

/// Turn the lock off: persist, stop the persister, remove the encrypted artifacts, and run on
/// the (already-present) plaintext working file. Requires the current password.
#[tauri::command]
pub fn disable_lock(
    app: AppHandle,
    db: State<Db>,
    datakey: State<DataKey>,
    mut password: String,
) -> Result<(), String> {
    let dir = data_dir(&app)?;
    let manifest = lock::read(&dir).ok_or("App lock isn't configured.")?;
    lock::unlock(&manifest, &password).map_err(|_| "Password is incorrect.".to_string())?;
    password.zeroize();

    let _ = db.persist_now_blocking();
    db.disable_encryption();
    // Convert secrets back to plaintext while the data key is still available —
    // afterwards there would be nothing left to decrypt them with, and every saved
    // server would fail to connect.
    secrets::rekey_all(&secrets::all_secret_keys(&db), None);
    let _ = secrets::clear_data_key();
    let enc = enc_path(&dir);
    let _ = std::fs::remove_file(&enc);
    let _ = std::fs::remove_file(enc.with_extension("clean"));
    let _ = std::fs::remove_file(lock::manifest_path(&dir));
    *datakey.0.lock().unwrap() = None;
    Ok(())
}

/// Export a PLAINTEXT copy of the DB (the user's own escape hatch — there is no recovery key).
/// Writes to the app data dir and returns the path so the UI can show where it went.
///
/// Requires the master password when a lock is configured. Without that check this
/// command is a one-call bypass of the entire lock: anything able to reach the IPC
/// bridge — including script injected into the webview — could drop a decrypted copy of
/// every server, chat and setting next to the encrypted blob, and nothing ever deletes
/// it. The password is verified against the manifest, exactly as `disable_lock` does.
#[tauri::command]
pub fn export_unencrypted_backup(
    app: AppHandle,
    db: State<Db>,
    mut password: String,
) -> Result<String, String> {
    let dir = data_dir(&app)?;
    if let Some(manifest) = lock::read(&dir) {
        let verdict = lock::unlock(&manifest, &password);
        password.zeroize();
        verdict.map_err(|_| "Password is incorrect.".to_string())?;
    } else {
        password.zeroize();
    }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dest = dir.join(format!("xconsole-unencrypted-backup-{stamp}.db"));
    db.backup_to(&dest).map_err(|e| format!("export failed: {e}"))?;
    Ok(dest.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    fn db() -> Db {
        Db::open_locked().unwrap()
    }

    /// The default has to be ON when the device is NOT remembered. An idle unlocked app
    /// is how credentials get used by someone who is not the user, and a protection
    /// nobody finds in Settings is not one.
    #[test]
    fn auto_lock_is_on_by_default_on_an_unremembered_device() {
        let d = db();
        assert_eq!(
            auto_lock_timeout_for(&d, false),
            Some(std::time::Duration::from_secs(AUTO_LOCK_DEFAULT_MINUTES * 60))
        );
    }

    /// ...and OFF by default when it is. The key is already in the keychain and the app
    /// unlocks itself at launch, so an idle lock is defeated by relaunching the app —
    /// while costing every live SSH, SFTP and database session. Reported as "it
    /// disconnects me when I leave it in the background even though I ticked remember".
    #[test]
    fn a_remembered_device_does_not_idle_lock_by_default() {
        let d = db();
        assert_eq!(auto_lock_timeout_for(&d, true), None);
    }

    /// But an explicit choice wins, in both directions. Someone who sets 15 minutes on a
    /// remembered device means it, and the remembered case must not quietly ignore them.
    #[test]
    fn an_explicit_timeout_beats_the_remembered_default() {
        let d = db();
        d.set_setting(AUTO_LOCK_SETTING, "15").unwrap();
        assert_eq!(
            auto_lock_timeout_for(&d, true),
            Some(std::time::Duration::from_secs(15 * 60))
        );
        d.set_setting(AUTO_LOCK_SETTING, "0").unwrap();
        assert_eq!(auto_lock_timeout_for(&d, false), None);
    }

    #[test]
    fn zero_means_off_and_absurd_values_are_clamped() {
        let d = db();
        d.set_setting(AUTO_LOCK_SETTING, "0").unwrap();
        assert_eq!(auto_lock_timeout_for(&d, false), None, "0 must disable it outright");

        // A 30-day timeout is "off" wearing a costume; clamp it to something honest.
        d.set_setting(AUTO_LOCK_SETTING, "43200").unwrap();
        assert_eq!(
            auto_lock_timeout_for(&d, false),
            Some(std::time::Duration::from_secs(AUTO_LOCK_MAX_MINUTES * 60))
        );

        // Garbage in the setting must not silently disable the lock.
        d.set_setting(AUTO_LOCK_SETTING, "not-a-number").unwrap();
        assert_eq!(
            auto_lock_timeout_for(&d, false),
            Some(std::time::Duration::from_secs(AUTO_LOCK_DEFAULT_MINUTES * 60))
        );
    }

    /// Credential encryption must be on unless explicitly refused — the failure mode of
    /// defaulting off is a keychain full of directly usable passwords.
    #[test]
    fn credential_encryption_defaults_on_and_only_an_explicit_no_disables_it() {
        let d = db();
        assert!(crate::secrets::encryption_opted_in(&d), "default must be on");

        d.set_setting(crate::secrets::ENCRYPT_SECRETS_SETTING, "false")
            .unwrap();
        assert!(!crate::secrets::encryption_opted_in(&d));

        d.set_setting(crate::secrets::ENCRYPT_SECRETS_SETTING, "true")
            .unwrap();
        assert!(crate::secrets::encryption_opted_in(&d));
    }

    #[test]
    fn idle_timer_resets_on_activity() {
        let a = AutoLock::default();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let before = a.idle_for();
        a.touch();
        assert!(a.idle_for() < before, "touch() must reset the idle clock");
    }

    /// Without a heartbeat we are blind, not idle. Auto-locking anyway fires on a fixed
    /// schedule while the app is being used — which is what "it locks again and again"
    /// looks like from the outside.
    #[test]
    fn auto_lock_holds_off_until_the_ui_heartbeat_arrives() {
        let a = AutoLock::default();
        assert!(
            !a.can_measure_idleness(),
            "a fresh app has heard nothing from the UI yet"
        );
        // Unlocking resets the clock but is NOT proof the heartbeat works.
        a.touch();
        assert!(!a.can_measure_idleness());
        a.beat();
        assert!(a.can_measure_idleness(), "note_activity proves it works");
    }
}

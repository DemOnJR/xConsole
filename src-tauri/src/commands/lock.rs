//! App-lock commands: master-password setup/unlock, change-password, forget-device, the
//! disable path, and the unencrypted-backup escape hatch. The encryption ENGINE is in
//! `storage/encrypt.rs`; these wire it to the user. No recovery by design — a forgotten
//! password with no remembered device means the data is unrecoverable.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
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
pub fn lock_status(app: AppHandle, datakey: State<DataKey>) -> Result<LockStatus, String> {
    let dir = data_dir(&app)?;
    Ok(LockStatus {
        enabled: lock::is_lock_enabled(&dir),
        unlocked: datakey.0.lock().unwrap().is_some(),
        remembered: secrets::get_data_key().ok().flatten().is_some(),
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
    mut password: String,
    remember: bool,
) -> Result<(), String> {
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
#[tauri::command]
pub fn export_unencrypted_backup(app: AppHandle, db: State<Db>) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dest = dir.join(format!("xconsole-unencrypted-backup-{stamp}.db"));
    db.backup_to(&dest).map_err(|e| format!("export failed: {e}"))?;
    Ok(dest.to_string_lossy().into_owned())
}

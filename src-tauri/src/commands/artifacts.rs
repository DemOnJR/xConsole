use std::path::Path;

use tauri::{AppHandle, Manager, State};

use crate::artifacts::{self, Artifact};
use crate::storage::Db;

#[tauri::command]
pub fn list_artifacts(db: State<'_, Db>, query: Option<String>) -> Result<Vec<Artifact>, String> {
    db.list_artifacts(query.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn verify_artifact(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let art = db
        .get_artifact(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "artifact not found".to_string())?;
    artifacts::verify_file(Path::new(&art.path), &art.sha256)
}

#[tauri::command]
pub fn reveal_artifact(db: State<'_, Db>, id: String) -> Result<(), String> {
    let art = db
        .get_artifact(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "artifact not found".to_string())?;
    reveal_path(&art.path)
}

#[tauri::command]
pub fn delete_artifact(db: State<'_, Db>, id: String) -> Result<(), String> {
    let Some(art) = db.delete_artifact(&id).map_err(|e| e.to_string())? else {
        return Err("artifact not found".into());
    };
    if art.kind == "ssh_key" || art.kind == "ssh_pub" {
        // Leave the key files on disk — deleting the backup would lock the user out.
        return Ok(());
    }
    let _ = std::fs::remove_file(&art.path);
    Ok(())
}

#[tauri::command]
pub fn artifacts_dir(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let root = artifacts::artifacts_root(&dir);
    let _ = std::fs::create_dir_all(&root);
    Ok(root.to_string_lossy().into_owned())
}

fn reveal_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("file is missing on disk".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = p.parent().unwrap_or(p);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

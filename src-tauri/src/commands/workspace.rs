use tauri::State;

use crate::storage::models::{KnownHost, Workspace, WorkspaceInput};
use crate::storage::Db;

#[tauri::command]
pub fn list_workspaces(db: State<'_, Db>) -> Result<Vec<Workspace>, String> {
    db.list_workspaces().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_workspace(db: State<'_, Db>, input: WorkspaceInput) -> Result<Workspace, String> {
    db.upsert_workspace(&input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workspace(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_workspace(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_known_hosts(db: State<'_, Db>) -> Result<Vec<KnownHost>, String> {
    db.list_known_hosts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn forget_host_key(db: State<'_, Db>, host: String, port: u16) -> Result<(), String> {
    db.forget_host_key(&host, port).map_err(|e| e.to_string())
}

//! Tauri commands for cloud provider accounts.

use tauri::State;

use crate::secrets;
use crate::storage::models::{CloudAccount, CloudAccountInput};
use crate::storage::Db;

#[tauri::command]
pub fn list_cloud_accounts(db: State<'_, Db>) -> Result<Vec<CloudAccount>, String> {
    db.list_cloud_accounts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_cloud_account(
    db: State<'_, Db>,
    input: CloudAccountInput,
) -> Result<CloudAccount, String> {
    let secret = input.secret.clone();
    let account = db.upsert_cloud_account(&input).map_err(|e| e.to_string())?;
    if let Some(secret) = secret {
        let key = secrets::cloud_account_key(&account.id);
        if secret.is_empty() {
            let _ = secrets::delete_secret(&key);
        } else {
            secrets::set_secret(&key, &secret).map_err(|e| e.to_string())?;
        }
    }
    db.get_cloud_account(&account.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "cloud account vanished after save".to_string())
}

#[tauri::command]
pub fn delete_cloud_account(db: State<'_, Db>, id: String) -> Result<(), String> {
    let _ = secrets::delete_secret(&secrets::cloud_account_key(&id));
    db.delete_cloud_account(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tfc_workspaces(
    db: State<'_, Db>,
    account_id: String,
) -> Result<Vec<String>, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "cloud account not found".to_string())?;
    if account.kind != "tfc" {
        return Err("account is not Terraform Cloud (kind must be tfc)".into());
    }
    let token = crate::infra::tfc::load_tfc_token(&account.id)?;
    crate::infra::tfc::list_workspaces(&account, &token).await
}

#[tauri::command]
pub async fn list_cloud_resources(
    db: State<'_, Db>,
    account_id: String,
    resource: Option<String>,
) -> Result<String, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "cloud account not found".to_string())?;
    let kind = account.kind.as_str();
    let resource = resource.as_deref().unwrap_or("all");
    match kind {
        "aws" => crate::infra::aws::list_resources(&account, resource).await,
        "gcp" => crate::infra::gcp::list_resources(&account, resource).await,
        "tfc" => Err("use list_tfc_workspaces for Terraform Cloud accounts".into()),
        other => Err(format!("unsupported cloud kind '{other}'")),
    }
}

//! Tauri commands for Cloudflare integration (1-Click Login, Tunnels, DNS, Security).

use tauri::State;

use crate::infra::cloudflare::{
    self, CfDnsRecord, CfDnsRecordInput, CfSecuritySettings, CfTunnel, CfTunnelConfig,
    CfZone,
};
use crate::storage::models::CloudAccount;
use crate::storage::Db;

#[tauri::command]
pub async fn start_cloudflare_oauth_login(db: State<'_, Db>) -> Result<String, String> {
    cloudflare::start_oauth_listener((*db).clone()).await
}

#[tauri::command]
pub async fn save_cloudflare_manual_token(
    db: State<'_, Db>,
    token: String,
) -> Result<CloudAccount, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("API Token cannot be empty".into());
    }
    let name = cloudflare::handle_token_login(&db, token).await?;
    let accounts = db.list_cloud_accounts().map_err(|e| e.to_string())?;
    accounts
        .into_iter()
        .find(|a| a.kind == "cloudflare" && a.name.contains(&name))
        .ok_or_else(|| "Saved Cloudflare account not found".into())
}

#[tauri::command]
pub async fn list_cloudflare_zones(
    db: State<'_, Db>,
    account_id: String,
) -> Result<Vec<CfZone>, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::list_zones(&token, account.project_id.as_deref()).await
}

#[tauri::command]
pub async fn list_cloudflare_tunnels(
    db: State<'_, Db>,
    account_id: String,
) -> Result<Vec<CfTunnel>, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::list_tunnels(&token, &cf_acc_id).await
}

#[tauri::command]
pub async fn create_cloudflare_tunnel(
    db: State<'_, Db>,
    account_id: String,
    name: String,
) -> Result<CfTunnel, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::create_tunnel(&token, &cf_acc_id, &name).await
}

#[tauri::command]
pub async fn delete_cloudflare_tunnel(
    db: State<'_, Db>,
    account_id: String,
    tunnel_id: String,
) -> Result<(), String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::delete_tunnel(&token, &cf_acc_id, &tunnel_id).await
}

#[tauri::command]
pub async fn get_cloudflare_tunnel_config(
    db: State<'_, Db>,
    account_id: String,
    tunnel_id: String,
) -> Result<CfTunnelConfig, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::get_tunnel_config(&token, &cf_acc_id, &tunnel_id).await
}

#[tauri::command]
pub async fn save_cloudflare_tunnel_config(
    db: State<'_, Db>,
    account_id: String,
    tunnel_id: String,
    config: CfTunnelConfig,
) -> Result<CfTunnelConfig, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::save_tunnel_config(&token, &cf_acc_id, &tunnel_id, &config).await
}

#[tauri::command]
pub async fn get_cloudflare_tunnel_token(
    db: State<'_, Db>,
    account_id: String,
    tunnel_id: String,
) -> Result<String, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    let cf_acc_id = account
        .project_id
        .ok_or_else(|| "Account is missing Cloudflare Account ID".to_string())?;
    cloudflare::get_tunnel_token(&token, &cf_acc_id, &tunnel_id).await
}

#[tauri::command]
pub async fn list_cloudflare_dns_records(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
) -> Result<Vec<CfDnsRecord>, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::list_dns_records(&token, &zone_id).await
}

#[tauri::command]
pub async fn upsert_cloudflare_dns_record(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
    record: CfDnsRecordInput,
) -> Result<CfDnsRecord, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::upsert_dns_record(&token, &zone_id, &record).await
}

#[tauri::command]
pub async fn delete_cloudflare_dns_record(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
    record_id: String,
) -> Result<(), String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::delete_dns_record(&token, &zone_id, &record_id).await
}

#[tauri::command]
pub async fn get_cloudflare_security_settings(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
) -> Result<CfSecuritySettings, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::get_security_settings(&token, &zone_id).await
}

#[tauri::command]
pub async fn set_cloudflare_security_level(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
    level: String,
) -> Result<String, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::set_security_level(&token, &zone_id, &level).await
}

//! Tauri commands for Cloudflare integration (1-Click Login, Tunnels, DNS, Security).

use tauri::State;

use crate::infra::cloudflare::{
    self, CfDnsRecord, CfDnsRecordInput, CfSecuritySettings, CfTunnel, CfTunnelConfig,
    CfZone,
};
use crate::storage::models::{CloudAccount, CloudflareAuditLog, CloudflareAuditLogInput};
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

async fn resolve_cf_account_id(
    account: &crate::storage::models::CloudAccount,
    token: &str,
) -> Result<String, String> {
    if let Some(pid) = &account.project_id {
        if !pid.trim().is_empty() {
            return Ok(pid.trim().to_string());
        }
    }
    if let Ok(accs) = cloudflare::list_accounts(token).await {
        if let Some(first) = accs.first() {
            return Ok(first.id.clone());
        }
    }
    if let Ok(zones) = cloudflare::list_zones(token, None).await {
        for z in zones {
            if let Some(acc) = z.account {
                return Ok(acc.id);
            }
        }
    }
    Err("Nu a putut fi determinat Cloudflare Account ID din token. Asigură-te că token-ul are permisiune pentru Cloudflare Tunnel sau cel puțin o Zonă DNS asociată.".to_string())
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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
    let tunnel = cloudflare::create_tunnel(&token, &cf_acc_id, &name).await?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "create_tunnel".to_string(),
        target_id: Some(tunnel.id.clone()),
        target_name: Some(tunnel.name.clone()),
        summary: format!("Creat tunel Zero Trust: {}", tunnel.name),
        actor: "user".to_string(),
        session_id: None,
        before_state: None,
        after_state: serde_json::to_string(&tunnel).ok(),
    });

    Ok(tunnel)
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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "delete_tunnel".to_string(),
        target_id: Some(tunnel_id.clone()),
        target_name: None,
        summary: format!("Șters tunel Zero Trust ID: {}", tunnel_id),
        actor: "user".to_string(),
        session_id: None,
        before_state: None,
        after_state: None,
    });

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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;

    let old_config = cloudflare::get_tunnel_config(&token, &cf_acc_id, &tunnel_id).await.ok();
    let saved = cloudflare::save_tunnel_config(&token, &cf_acc_id, &tunnel_id, &config).await?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "update_tunnel_config".to_string(),
        target_id: Some(tunnel_id.clone()),
        target_name: Some(format!("tunel_{}", &tunnel_id[..8.min(tunnel_id.len())])),
        summary: format!("Actualizat rute de ingress pentru tunelul {}", &tunnel_id[..8.min(tunnel_id.len())]),
        actor: "user".to_string(),
        session_id: None,
        before_state: old_config.and_then(|c| serde_json::to_string(&c).ok()),
        after_state: serde_json::to_string(&saved).ok(),
    });

    Ok(saved)
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
    let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
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

    // Snapshot existing record for rollback
    let mut before_state: Option<String> = None;
    if let Some(rec_id) = &record.id {
        if !rec_id.is_empty() {
            if let Ok(records) = cloudflare::list_dns_records(&token, &zone_id).await {
                if let Some(existing) = records.into_iter().find(|r| r.id == *rec_id) {
                    let input_snap = CfDnsRecordInput {
                        id: Some(existing.id),
                        name: existing.name,
                        r#type: existing.r#type,
                        content: existing.content,
                        proxied: existing.proxied,
                        ttl: existing.ttl,
                        comment: existing.comment,
                    };
                    before_state = serde_json::to_string(&input_snap).ok();
                }
            }
        }
    }

    let saved = cloudflare::upsert_dns_record(&token, &zone_id, &record).await?;

    let is_update = before_state.is_some();
    let summary = if is_update {
        format!("Actualizat DNS {} {} &rarr; {}", saved.r#type, saved.name, saved.content)
    } else {
        format!("Creat DNS nou: {} {} ({})", saved.r#type, saved.name, saved.content)
    };

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: if is_update { "update_dns".to_string() } else { "create_dns".to_string() },
        target_id: Some(saved.id.clone()),
        target_name: Some(saved.name.clone()),
        summary,
        actor: "user".to_string(),
        session_id: None,
        before_state,
        after_state: serde_json::to_string(&saved).ok(),
    });

    Ok(saved)
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

    // Snapshot before delete for rollback
    let mut before_state: Option<String> = None;
    let mut record_name = record_id.clone();
    if let Ok(records) = cloudflare::list_dns_records(&token, &zone_id).await {
        if let Some(existing) = records.into_iter().find(|r| r.id == record_id) {
            record_name = existing.name.clone();
            let input_snap = CfDnsRecordInput {
                id: None, // On revert, we re-create as new record
                name: existing.name,
                r#type: existing.r#type,
                content: existing.content,
                proxied: existing.proxied,
                ttl: existing.ttl,
                comment: existing.comment,
            };
            before_state = serde_json::to_string(&input_snap).ok();
        }
    }

    cloudflare::delete_dns_record(&token, &zone_id, &record_id).await?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "delete_dns".to_string(),
        target_id: Some(record_id),
        target_name: Some(record_name.clone()),
        summary: format!("Șters înregistrare DNS: {}", record_name),
        actor: "user".to_string(),
        session_id: None,
        before_state,
        after_state: None,
    });

    Ok(())
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

    let old_settings = cloudflare::get_security_settings(&token, &zone_id).await.ok();
    let old_level = old_settings.map(|s| s.security_level);

    let updated = cloudflare::set_security_level(&token, &zone_id, &level).await?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "set_security_level".to_string(),
        target_id: Some(zone_id),
        target_name: Some("WAF Security Level".to_string()),
        summary: format!("Modificat nivel securitate WAF: {}", level),
        actor: "user".to_string(),
        session_id: None,
        before_state: old_level,
        after_state: Some(level),
    });

    Ok(updated)
}

#[tauri::command]
pub async fn list_cloudflare_history(
    db: State<'_, Db>,
    account_id: String,
) -> Result<Vec<CloudflareAuditLog>, String> {
    db.list_cloudflare_audit_logs(&account_id, 100).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn revert_cloudflare_action(
    db: State<'_, Db>,
    account_id: String,
    log_id: String,
) -> Result<String, String> {
    let log = db
        .get_cloudflare_audit_log(&log_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Istoricul modificării nu a fost găsit".to_string())?;

    if log.reverted {
        return Err("Această acțiune a fost deja anulată (reverted)".into());
    }

    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;

    match log.action_type.as_str() {
        "create_dns" => {
            // Revert of creation is deletion
            if let Some(target_id) = &log.target_id {
                let zone_id = account.region.clone().unwrap_or_default();
                if !zone_id.is_empty() {
                    let _ = cloudflare::delete_dns_record(&token, &zone_id, target_id).await;
                } else if let Ok(zones) = cloudflare::list_zones(&token, account.project_id.as_deref()).await {
                    for z in zones {
                        if cloudflare::delete_dns_record(&token, &z.id, target_id).await.is_ok() {
                            break;
                        }
                    }
                }
            }
        }
        "update_dns" | "delete_dns" => {
            // Restore previous DNS record state
            if let Some(before_json) = &log.before_state {
                let snap: CfDnsRecordInput = serde_json::from_str(before_json)
                    .map_err(|e| format!("Eroare la parsare stare anterioară DNS: {e}"))?;
                let zone_id = account.region.clone().unwrap_or_default();
                let actual_zone = if !zone_id.is_empty() {
                    zone_id
                } else {
                    let zones = cloudflare::list_zones(&token, account.project_id.as_deref()).await?;
                    zones.first().map(|z| z.id.clone()).unwrap_or_default()
                };
                if !actual_zone.is_empty() {
                    cloudflare::upsert_dns_record(&token, &actual_zone, &snap).await?;
                }
            }
        }
        "set_security_level" => {
            if let Some(prev_level) = &log.before_state {
                if let Some(zone_id) = &log.target_id {
                    cloudflare::set_security_level(&token, zone_id, prev_level).await?;
                }
            }
        }
        "update_tunnel_config" => {
            if let (Some(tunnel_id), Some(prev_config_json)) = (&log.target_id, &log.before_state) {
                let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
                let prev_config: CfTunnelConfig = serde_json::from_str(prev_config_json)
                    .map_err(|e| format!("Eroare la parsare configurare tunel: {e}"))?;
                cloudflare::save_tunnel_config(&token, &cf_acc_id, tunnel_id, &prev_config).await?;
            }
        }
        "create_tunnel" => {
            if let Some(tunnel_id) = &log.target_id {
                let cf_acc_id = resolve_cf_account_id(&account, &token).await?;
                cloudflare::delete_tunnel(&token, &cf_acc_id, tunnel_id).await?;
            }
        }
        other => {
            return Err(format!("Tipul de acțiune '{other}' nu suportă rollback automat"));
        }
    }

    db.mark_cloudflare_audit_log_reverted(&log_id).map_err(|e| e.to_string())?;

    let _ = db.create_cloudflare_audit_log(&CloudflareAuditLogInput {
        account_id: account_id.clone(),
        action_type: "revert_action".to_string(),
        target_id: Some(log_id.clone()),
        target_name: log.target_name.clone(),
        summary: format!("↩️ Anulat (Rollback): {}", log.summary),
        actor: "user".to_string(),
        session_id: None,
        before_state: None,
        after_state: None,
    });

    Ok(format!("Modificarea '{}' a fost anulată cu succes!", log.summary))
}

#[tauri::command]
pub async fn get_cloudflare_zone_analytics(
    db: State<'_, Db>,
    account_id: String,
    zone_id: String,
    since_minutes: Option<i64>,
) -> Result<serde_json::Value, String> {
    let account = db
        .get_cloud_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Cloudflare account not found".to_string())?;
    let token = cloudflare::load_cf_token(&account.id)?;
    cloudflare::get_zone_analytics(&token, &zone_id, since_minutes).await
}

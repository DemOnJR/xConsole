use std::time::Duration;

use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::secrets;
use crate::storage::models::CloudAccountInput;
use crate::storage::Db;

const CF_API: &str = "https://api.cloudflare.com/client/v4";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfVerifyResponse {
    pub id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfAccount {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfZone {
    pub id: String,
    pub name: String,
    pub status: String,
    pub paused: bool,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfTunnel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub conns_active_at: Option<String>,
    #[serde(default)]
    pub connections: Vec<CfTunnelConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfTunnelConnection {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub colo_name: Option<String>,
    #[serde(default)]
    pub is_pending_reconnect: Option<bool>,
    #[serde(default)]
    pub opened_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfIngressRule {
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    pub service: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfTunnelConfig {
    #[serde(default)]
    pub ingress: Vec<CfIngressRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfDnsRecord {
    pub id: String,
    pub zone_id: String,
    pub zone_name: String,
    pub name: String,
    pub r#type: String,
    pub content: String,
    pub proxiable: bool,
    pub proxied: bool,
    pub ttl: u32,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub created_on: Option<String>,
    #[serde(default)]
    pub modified_on: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfDnsRecordInput {
    pub id: Option<String>,
    pub name: String,
    pub r#type: String,
    pub content: String,
    pub proxied: bool,
    #[serde(default = "default_ttl")]
    pub ttl: u32,
    #[serde(default)]
    pub comment: Option<String>,
}

fn default_ttl() -> u32 {
    1 // Auto TTL
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CfSecuritySettings {
    pub security_level: String,
    #[serde(default)]
    pub ssl: Option<String>,
    #[serde(default)]
    pub attack_mode: bool,
}

pub fn load_cf_token(account_id: &str) -> Result<String, String> {
    secrets::get_secret(&secrets::cloud_account_key(account_id))
        .map_err(|e| format!("Cloudflare token not in keychain: {e}"))?
        .map(|s| s.to_string())
        .ok_or_else(|| "No Cloudflare API token stored for this account".to_string())
}

fn make_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

// -----------------------------------------------------------------------------
// Core Cloudflare API Helpers
// -----------------------------------------------------------------------------

/// Verify that a Cloudflare API token is valid and active.
pub async fn verify_token(token: &str) -> Result<CfVerifyResponse, String> {
    let client = make_client();
    let res = client
        .get(format!("{CF_API}/user/tokens/verify"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Cloudflare token verification failed: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Null);
    let verify: CfVerifyResponse =
        serde_json::from_value(result).map_err(|e| format!("Failed to parse verify result: {e}"))?;
    Ok(verify)
}

/// List all accounts the token has access to.
pub async fn list_accounts(token: &str) -> Result<Vec<CfAccount>, String> {
    let client = make_client();
    let res = client
        .get(format!("{CF_API}/accounts?page=1&per_page=50"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to list Cloudflare accounts: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Array(vec![]));
    let accounts: Vec<CfAccount> = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse accounts list: {e}"))?;
    Ok(accounts)
}

/// List all DNS zones (domains) for an account.
pub async fn list_zones(token: &str, cf_account_id: Option<&str>) -> Result<Vec<CfZone>, String> {
    let client = make_client();
    let mut url = format!("{CF_API}/zones?page=1&per_page=50");
    if let Some(acc_id) = cf_account_id {
        if !acc_id.is_empty() {
            url.push_str(&format!("&account.id={acc_id}"));
        }
    }

    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to list Cloudflare zones: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Array(vec![]));
    let zones: Vec<CfZone> = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse zones list: {e}"))?;
    Ok(zones)
}

// -----------------------------------------------------------------------------
// Cloudflare Tunnels (Zero Trust / Argo Tunnels)
// -----------------------------------------------------------------------------

/// List all active and inactive tunnels for a Cloudflare account.
pub async fn list_tunnels(token: &str, cf_account_id: &str) -> Result<Vec<CfTunnel>, String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel?is_deleted=false&per_page=50");

    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to list Cloudflare tunnels: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Array(vec![]));
    let tunnels: Vec<CfTunnel> = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse tunnels list: {e}"))?;
    Ok(tunnels)
}

/// Create a new Cloudflare Tunnel.
pub async fn create_tunnel(
    token: &str,
    cf_account_id: &str,
    name: &str,
) -> Result<CfTunnel, String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel");

    // Generate a random 32-byte tunnel secret (base64)
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut secret_bytes = [0u8; 32];
    rng.fill(&mut secret_bytes)
        .map_err(|_| "Failed to generate random tunnel secret".to_string())?;
    let tunnel_secret = base64::engine::general_purpose::STANDARD.encode(secret_bytes);

    let body = json!({
        "name": name,
        "tunnel_secret": tunnel_secret,
        "config_src": "cloudflare"
    });

    let res = client
        .post(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to create Cloudflare tunnel: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Null);
    let tunnel: CfTunnel = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse created tunnel: {e}"))?;
    Ok(tunnel)
}

/// Delete a Cloudflare Tunnel.
pub async fn delete_tunnel(
    token: &str,
    cf_account_id: &str,
    tunnel_id: &str,
) -> Result<(), String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel/{tunnel_id}");

    let res = client
        .delete(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to delete Cloudflare tunnel: {errs}"));
    }

    Ok(())
}

/// Get the configuration (ingress rules) for a Cloudflare tunnel.
pub async fn get_tunnel_config(
    token: &str,
    cf_account_id: &str,
    tunnel_id: &str,
) -> Result<CfTunnelConfig, String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel/{tunnel_id}/configurations");

    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to fetch tunnel configuration: {errs}"));
    }

    let config_val = json
        .get("result")
        .and_then(|r| r.get("config"))
        .cloned()
        .unwrap_or_else(|| json!({ "ingress": [] }));

    let config: CfTunnelConfig = serde_json::from_value(config_val)
        .unwrap_or_else(|_| CfTunnelConfig { ingress: vec![] });
    Ok(config)
}

/// Update the configuration (ingress rules) for a Cloudflare tunnel.
pub async fn save_tunnel_config(
    token: &str,
    cf_account_id: &str,
    tunnel_id: &str,
    config: &CfTunnelConfig,
) -> Result<CfTunnelConfig, String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel/{tunnel_id}/configurations");

    let mut ingress = config.ingress.clone();
    // Ensure there is always a catch-all 404 rule at the end
    let has_catch_all = ingress
        .iter()
        .any(|r| r.hostname.is_none() || r.service.starts_with("http_status:"));
    if !has_catch_all {
        ingress.push(CfIngressRule {
            hostname: None,
            path: None,
            service: "http_status:404".to_string(),
        });
    }

    let body = json!({
        "config": {
            "ingress": ingress
        }
    });

    let res = client
        .put(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to update tunnel configuration: {errs}"));
    }

    let config_val = json
        .get("result")
        .and_then(|r| r.get("config"))
        .cloned()
        .unwrap_or_else(|| json!({ "ingress": [] }));

    let updated: CfTunnelConfig = serde_json::from_value(config_val)
        .map_err(|e| format!("Failed to parse updated tunnel configuration: {e}"))?;
    Ok(updated)
}

/// Retrieve the cloudflared run token for a tunnel (`cloudflared run --token <TOKEN>`).
pub async fn get_tunnel_token(
    token: &str,
    cf_account_id: &str,
    tunnel_id: &str,
) -> Result<String, String> {
    let client = make_client();
    let url = format!("{CF_API}/accounts/{cf_account_id}/cfd_tunnel/{tunnel_id}/token");

    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to get tunnel token: {errs}"));
    }

    let token_str = json
        .get("result")
        .and_then(|r| r.as_str())
        .ok_or_else(|| "Missing token string in response".to_string())?;

    Ok(token_str.to_string())
}

// -----------------------------------------------------------------------------
// DNS Records Management
// -----------------------------------------------------------------------------

/// List DNS records for a given zone.
pub async fn list_dns_records(token: &str, zone_id: &str) -> Result<Vec<CfDnsRecord>, String> {
    let client = make_client();
    let url = format!("{CF_API}/zones/{zone_id}/dns_records?page=1&per_page=100");

    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to list DNS records: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Array(vec![]));
    let records: Vec<CfDnsRecord> = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse DNS records: {e}"))?;
    Ok(records)
}

/// Create or update a DNS record.
pub async fn upsert_dns_record(
    token: &str,
    zone_id: &str,
    input: &CfDnsRecordInput,
) -> Result<CfDnsRecord, String> {
    let client = make_client();
    let body = json!({
        "type": input.r#type,
        "name": input.name,
        "content": input.content,
        "proxied": input.proxied,
        "ttl": input.ttl,
        "comment": input.comment
    });

    let res = if let Some(rec_id) = &input.id {
        if !rec_id.is_empty() {
            // Update existing
            let url = format!("{CF_API}/zones/{zone_id}/dns_records/{rec_id}");
            client.put(&url).bearer_auth(token).json(&body).send().await
        } else {
            // Create new
            let url = format!("{CF_API}/zones/{zone_id}/dns_records");
            client.post(&url).bearer_auth(token).json(&body).send().await
        }
    } else {
        // Create new
        let url = format!("{CF_API}/zones/{zone_id}/dns_records");
        client.post(&url).bearer_auth(token).json(&body).send().await
    }
    .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to save DNS record: {errs}"));
    }

    let result = json.get("result").cloned().unwrap_or(Value::Null);
    let record: CfDnsRecord = serde_json::from_value(result)
        .map_err(|e| format!("Failed to parse DNS record: {e}"))?;
    Ok(record)
}

/// Delete a DNS record.
pub async fn delete_dns_record(
    token: &str,
    zone_id: &str,
    record_id: &str,
) -> Result<(), String> {
    let client = make_client();
    let url = format!("{CF_API}/zones/{zone_id}/dns_records/{record_id}");

    let res = client
        .delete(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to delete DNS record: {errs}"));
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Security & WAF Settings
// -----------------------------------------------------------------------------

/// Get security settings for a zone (Security level, Under attack mode, SSL level).
pub async fn get_security_settings(
    token: &str,
    zone_id: &str,
) -> Result<CfSecuritySettings, String> {
    let client = make_client();

    let sec_url = format!("{CF_API}/zones/{zone_id}/settings/security_level");
    let sec_res = client
        .get(&sec_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let sec_json: Value = sec_res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    let sec_level = sec_json
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("medium")
        .to_string();

    let ssl_url = format!("{CF_API}/zones/{zone_id}/settings/ssl");
    let ssl_res = client.get(&ssl_url).bearer_auth(token).send().await;
    let ssl_val = if let Ok(resp) = ssl_res {
        if let Ok(json) = resp.json::<Value>().await {
            json.get("result")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    Ok(CfSecuritySettings {
        attack_mode: sec_level == "under_attack",
        security_level: sec_level,
        ssl: ssl_val,
    })
}

/// Set security level for a zone ("essentially_off", "low", "medium", "high", "under_attack").
pub async fn set_security_level(
    token: &str,
    zone_id: &str,
    level: &str,
) -> Result<String, String> {
    let client = make_client();
    let url = format!("{CF_API}/zones/{zone_id}/settings/security_level");

    let body = json!({ "value": level });

    let res = client
        .patch(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let json: Value = res
        .json()
        .await
        .map_err(|e| format!("Invalid JSON response: {e}"))?;

    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        let errs = json
            .get("errors")
            .map(|e| e.to_string())
            .unwrap_or_else(|| "Unknown error".into());
        return Err(format!("Failed to update security level: {errs}"));
    }

    let updated_level = json
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or(level)
        .to_string();

    Ok(updated_level)
}

// -----------------------------------------------------------------------------
// 1-Click Browser Login Server
// -----------------------------------------------------------------------------

/// Start a lightweight local loopback HTTP server to handle the 1-Click Cloudflare Login.
/// Listens on `127.0.0.1:<random_port>` and returns the allocated port.
pub async fn start_oauth_listener(db: Db) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind local loopback listener: {e}"))?;

    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local port: {e}"))?
        .port();

    tokio::spawn(async move {
        // Wait for connection with a 5-minute timeout
        let timeout = tokio::time::sleep(Duration::from_secs(300));
        tokio::pin!(timeout);

        tokio::select! {
            accept_res = listener.accept() => {
                if let Ok((mut stream, _)) = accept_res {
                    let mut buffer = [0u8; 4096];
                    if let Ok(n) = stream.read(&mut buffer).await {
                        let req = String::from_utf8_lossy(&buffer[..n]);

                        // Handle token extraction from either GET query or POST JSON/form
                        let token_opt = extract_token_from_request(&req);

                        if let Some(token) = token_opt {
                            match handle_token_login(&db, &token).await {
                                Ok(account_name) => {
                                    let html = format!(
                                        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>xConsole &bull; Cloudflare Connected</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #090d16;
            color: #e2e8f0;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            margin: 0;
        }}
        .card {{
            background: #131b2e;
            border: 1px solid #23304a;
            border-radius: 12px;
            padding: 32px;
            text-align: center;
            max-width: 420px;
            box-shadow: 0 20px 40px rgba(0,0,0,0.5);
        }}
        .badge {{
            display: inline-block;
            background: #f48120;
            color: white;
            font-size: 11px;
            font-weight: 700;
            padding: 4px 10px;
            border-radius: 9999px;
            margin-bottom: 16px;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }}
        h1 {{ font-size: 20px; margin: 0 0 8px 0; font-weight: 600; color: #fff; }}
        p {{ font-size: 14px; color: #94a3b8; line-height: 1.5; margin: 0 0 24px 0; }}
        .done {{ color: #10b981; font-weight: 500; font-size: 13px; }}
    </style>
</head>
<body>
    <div class="card">
        <div class="badge">Cloudflare</div>
        <h1>Conectat cu succes!</h1>
        <p>Contul <strong>{}</strong> a fost asociat cu xConsole.</p>
        <div class="done">&check; Po&#539;i &icirc;nchide aceast&#259; fil&#259; &#537;i reveni &icirc;n aplica&#539;ie.</div>
    </div>
</body>
</html>"#,
                                        account_name
                                    );
                                    let response = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        html.len(),
                                        html
                                    );
                                    let _ = stream.write_all(response.as_bytes()).await;
                                }
                                Err(err) => {
                                    let html = format!(
                                        "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Eroare autentificare Cloudflare</h2><p>{}</p></body></html>",
                                        err
                                    );
                                    let response = format!(
                                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        html.len(),
                                        html
                                    );
                                    let _ = stream.write_all(response.as_bytes()).await;
                                }
                            }
                        } else {
                            // Serve landing page with helper token generation script
                            let html = format!(
                                r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>xConsole &bull; Autentificare Cloudflare</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background: #090d16;
            color: #e2e8f0;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            margin: 0;
            padding: 20px;
        }}
        .card {{
            background: #131b2e;
            border: 1px solid #23304a;
            border-radius: 12px;
            padding: 32px;
            max-width: 480px;
            box-shadow: 0 20px 40px rgba(0,0,0,0.5);
        }}
        .badge {{
            display: inline-block;
            background: #f48120;
            color: white;
            font-size: 11px;
            font-weight: 700;
            padding: 4px 10px;
            border-radius: 9999px;
            margin-bottom: 16px;
            text-transform: uppercase;
        }}
        h1 {{ font-size: 20px; margin: 0 0 12px 0; color: #fff; }}
        p {{ font-size: 13px; color: #94a3b8; line-height: 1.5; margin: 0 0 20px 0; }}
        .btn {{
            display: inline-block;
            background: #f48120;
            color: white;
            text-decoration: none;
            padding: 10px 18px;
            border-radius: 6px;
            font-weight: 500;
            font-size: 13px;
            cursor: pointer;
            border: none;
            width: 100%;
            box-sizing: border-box;
            text-align: center;
        }}
        .btn:hover {{ background: #e06d0e; }}
        .input-box {{
            margin-top: 16px;
            text-align: left;
        }}
        input {{
            width: 100%;
            background: #090d16;
            border: 1px solid #23304a;
            color: #fff;
            padding: 10px;
            border-radius: 6px;
            font-size: 13px;
            box-sizing: border-box;
            outline: none;
            margin-bottom: 10px;
        }}
        input:focus {{ border-color: #f48120; }}
    </style>
</head>
<body>
    <div class="card">
        <div class="badge">Cloudflare 1-Click Connect</div>
        <h1>Conectare Cloudflare la xConsole</h1>
        <p>1. Apas&#259; butonul de mai jos pentru a deschide pagina Cloudflare cu permisiuni preconfigurate (Tunnels, DNS, WAF &#537;i Securitate).<br>2. D&#259; click pe <strong>Continue to summary &rarr; Create Token</strong> &#537;i lipe&#537;te token-ul mai jos:</p>
        
        <a href="https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys=[%22dns%22,%22zone_settings%22,%22zone%22,%22waf%22,%22argo_tunnel%22]&name=xConsole" target="_blank" class="btn">
            Deschide Cloudflare Token Generator &rarr;
        </a>

        <form action="/callback" method="GET" class="input-box">
            <label style="font-size: 11px; color: #94a3b8; display: block; margin-bottom: 4px;">Lipe&#537;te API Token-ul aici:</label>
            <input type="password" name="token" placeholder="v4.0-..." required autofocus />
            <button type="submit" class="btn" style="background:#2563eb;">Finalizeaz&#259; conectarea &check;</button>
        </form>
    </div>
</body>
</html>"#
                            );
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                html.len(),
                                html
                            );
                            let _ = stream.write_all(response.as_bytes()).await;
                        }
                    }
                }
            }
            _ = timeout => {
                // Timeout after 5 minutes
            }
        }
    });

    Ok(port)
}

fn extract_token_from_request(req: &str) -> Option<String> {
    if let Some(pos) = req.find("token=") {
        let after = &req[pos + 6..];
        let token_raw = after.split(|c| c == '&' || c == ' ' || c == '\r' || c == '\n').next().unwrap_or("");
        let decoded = urlencoding_decode(token_raw);
        if !decoded.trim().is_empty() {
            return Some(decoded.trim().to_string());
        }
    }
    None
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            let hex_str = format!("{}{}", h1, h2);
            if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                out.push(byte as char);
            }
        } else if c == '+' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

pub async fn handle_token_login(db: &Db, token: &str) -> Result<String, String> {
    let _verify = verify_token(token).await?;
    let accounts = list_accounts(token).await?;

    let account = accounts
        .first()
        .ok_or_else(|| "Nu a fost găsit niciun cont asociat cu acest token Cloudflare".to_string())?;

    let zones = list_zones(token, Some(&account.id)).await.unwrap_or_default();
    let default_zone = zones.first().map(|z| z.id.clone());

    let input = CloudAccountInput {
        id: None,
        name: format!("Cloudflare ({})", account.name),
        kind: "cloudflare".to_string(),
        region: default_zone,
        project_id: Some(account.id.clone()),
        organization: Some(account.name.clone()),
        secret: Some(token.to_string()),
    };

    let saved = db.upsert_cloud_account(&input).map_err(|e| e.to_string())?;
    let key = secrets::cloud_account_key(&saved.id);
    secrets::set_secret(&key, token).map_err(|e| e.to_string())?;

    Ok(account.name.clone())
}

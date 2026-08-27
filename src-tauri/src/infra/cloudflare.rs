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

#[allow(dead_code)]
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

pub fn apply_auth(builder: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    let token = token.trim();
    if token.contains(':') && !token.starts_with("v4.0-") && token.contains('@') {
        let mut parts = token.splitn(2, ':');
        let email = parts.next().unwrap_or("").trim();
        let key = parts.next().unwrap_or("").trim();
        builder.header("X-Auth-Email", email).header("X-Auth-Key", key)
    } else if token.contains('\n') && token.contains('@') {
        let mut parts = token.lines();
        let email = parts.next().unwrap_or("").trim();
        let key = parts.next().unwrap_or("").trim();
        builder.header("X-Auth-Email", email).header("X-Auth-Key", key)
    } else {
        builder.bearer_auth(token)
    }
}

// -----------------------------------------------------------------------------
// Core Cloudflare API Helpers
// -----------------------------------------------------------------------------

/// Verify that a Cloudflare API token is valid and active.
#[allow(dead_code)]
pub async fn verify_token(token: &str) -> Result<CfVerifyResponse, String> {
    let client = make_client();
    let req = client.get(format!("{CF_API}/user/tokens/verify"));
    let res = apply_auth(req, token)
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
    let req = client.get(format!("{CF_API}/accounts?page=1&per_page=50"));
    let res = apply_auth(req, token)
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

    let req = client.get(&url);
    let res = apply_auth(req, token)
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

    let req = client.get(&url);
    let res = apply_auth(req, token)
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

    let req = client.post(&url).json(&body);
    let res = apply_auth(req, token)
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

    let req = client.delete(&url);
    let res = apply_auth(req, token)
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

    let req = client.get(&url);
    let res = apply_auth(req, token)
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

    let req = client.put(&url).json(&body);
    let res = apply_auth(req, token)
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

    let req = client.get(&url);
    let res = apply_auth(req, token)
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

    let req = client.get(&url);
    let res = apply_auth(req, token)
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
            let req = client.put(&url).json(&body);
            apply_auth(req, token).send().await
        } else {
            // Create new
            let url = format!("{CF_API}/zones/{zone_id}/dns_records");
            let req = client.post(&url).json(&body);
            apply_auth(req, token).send().await
        }
    } else {
        // Create new
        let url = format!("{CF_API}/zones/{zone_id}/dns_records");
        let req = client.post(&url).json(&body);
        apply_auth(req, token).send().await
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

    let req = client.delete(&url);
    let res = apply_auth(req, token)
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
    let sec_req = client.get(&sec_url);
    let sec_res = apply_auth(sec_req, token)
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
    let ssl_req = client.get(&ssl_url);
    let ssl_res = apply_auth(ssl_req, token).send().await;
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

    let req = client.patch(&url).json(&body);
    let res = apply_auth(req, token)
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
// 1-Click Cloudflare OAuth 2.0 PKCE Browser Login
// -----------------------------------------------------------------------------

const CF_OAUTH_CLIENT_ID: &str = "54d11594-84e4-41aa-b438-e81b8fa78ee7";
const CF_OAUTH_AUTH_URL: &str = "https://dash.cloudflare.com/oauth2/auth";
const CF_OAUTH_TOKEN_URL: &str = "https://dash.cloudflare.com/oauth2/token";

/// Start a lightweight local loopback HTTP server to handle the 1-Click Cloudflare OAuth 2.0 Login.
/// Listens on `localhost:8976` (the official registered port for Cloudflare OAuth) and returns the authorization URL.
pub async fn start_oauth_listener(db: Db) -> Result<String, String> {
    use ring::digest::{digest, SHA256};
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();

    // 1. Generate PKCE code_verifier (32 random bytes -> URL-safe base64 without padding)
    let mut verifier_bytes = [0u8; 32];
    rng.fill(&mut verifier_bytes)
        .map_err(|_| "Failed to generate random bytes for PKCE".to_string())?;
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);

    // 2. Compute PKCE code_challenge = Base64URL-Encode(SHA256(code_verifier))
    let challenge_hash = digest(&SHA256, code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_hash.as_ref());

    // 3. Generate random state for CSRF protection
    let mut state_bytes = [0u8; 16];
    rng.fill(&mut state_bytes)
        .map_err(|_| "Failed to generate state bytes".to_string())?;
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);

    // 4. Bind local listener on registered port 8976
    let listener = match TcpListener::bind("127.0.0.1:8976").await {
        Ok(l) => l,
        Err(_) => TcpListener::bind("0.0.0.0:8976").await.map_err(|e| {
            format!("Portul 8976 este deja utilizat de o altă instanță sau aplicație: {e}")
        })?,
    };

    let redirect_uri = "http://localhost:8976/oauth/callback".to_string();
    let encoded_redirect = urlencoding_encode(&redirect_uri);
    let scopes = "account:read user:read workers:write workers_kv:write workers_routes:write workers_scripts:write zone:read ssl_certs:write d1:write pages:write ai:write queues:write offline_access";
    let encoded_scopes = urlencoding_encode(scopes);

    let auth_url = format!(
        "{CF_OAUTH_AUTH_URL}?response_type=code&client_id={CF_OAUTH_CLIENT_ID}&redirect_uri={encoded_redirect}&scope={encoded_scopes}&state={state}&code_challenge={code_challenge}&code_challenge_method=S256"
    );

    let redirect_uri_clone = redirect_uri.clone();
    let code_verifier_clone = code_verifier.clone();
    let expected_state = state.clone();

    tokio::spawn(async move {
        // Wait for OAuth redirect callback with a 5-minute timeout
        let timeout = tokio::time::sleep(Duration::from_secs(300));
        tokio::pin!(timeout);

        tokio::select! {
            accept_res = listener.accept() => {
                if let Ok((mut stream, _)) = accept_res {
                    let mut buffer = [0u8; 4096];
                    if let Ok(n) = stream.read(&mut buffer).await {
                        let req = String::from_utf8_lossy(&buffer[..n]);

                        let code_opt = extract_query_param(&req, "code");
                        let state_opt = extract_query_param(&req, "state");

                        if let (Some(code), Some(ret_state)) = (code_opt, state_opt) {
                            if ret_state != expected_state {
                                let html = "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Eroare de securitate: state-ul nu se potrivește.</h2></body></html>";
                                let response = format!(
                                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    html.len(),
                                    html
                                );
                                let _ = stream.write_all(response.as_bytes()).await;
                                return;
                            }

                            // Exchange code for access_token with Cloudflare OAuth token endpoint
                            let client = make_client();
                            let token_params = [
                                ("grant_type", "authorization_code"),
                                ("client_id", CF_OAUTH_CLIENT_ID),
                                ("code", code.as_str()),
                                ("redirect_uri", redirect_uri_clone.as_str()),
                                ("code_verifier", code_verifier_clone.as_str()),
                            ];

                            let token_res = client
                                .post(CF_OAUTH_TOKEN_URL)
                                .form(&token_params)
                                .send()
                                .await;

                            match token_res {
                                Ok(resp) => {
                                    if let Ok(json) = resp.json::<Value>().await {
                                        let access_token = json
                                            .get("access_token")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();

                                        if !access_token.is_empty() {
                                            match handle_token_login(&db, &access_token).await {
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
        <p>Contul <strong>{}</strong> a fost asociat automat cu xConsole.</p>
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
                                                Err(e) => {
                                                    let html = format!(
                                                        "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Eroare salvare cont Cloudflare: {}</h2></body></html>",
                                                        e
                                                    );
                                                    let response = format!(
                                                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                                        html.len(),
                                                        html
                                                    );
                                                    let _ = stream.write_all(response.as_bytes()).await;
                                                }
                                            }
                                        } else {
                                            let err_msg = json
                                                .get("error_description")
                                                .or_else(|| json.get("error"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("Eroare necunoscută la generarea token-ului");
                                            let html = format!(
                                                "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Eroare OAuth Cloudflare</h2><p>{}</p></body></html>",
                                                err_msg
                                            );
                                            let response = format!(
                                                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                                html.len(),
                                                html
                                            );
                                            let _ = stream.write_all(response.as_bytes()).await;
                                        }
                                    }
                                }
                                Err(err) => {
                                    let html = format!(
                                        "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Eroare conexiune Cloudflare Token endpoint: {}</h2></body></html>",
                                        err
                                    );
                                    let response = format!(
                                        "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                        html.len(),
                                        html
                                    );
                                    let _ = stream.write_all(response.as_bytes()).await;
                                }
                            }
                        } else {
                            let html = "<!DOCTYPE html><html><body style='background:#090d16;color:#f87171;font-family:sans-serif;padding:40px;'><h2>Nu a fost primit niciun cod de autorizare.</h2></body></html>";
                            let response = format!(
                                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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

    Ok(auth_url)
}

fn extract_query_param(req: &str, param_name: &str) -> Option<String> {
    let search = format!("{param_name}=");
    if let Some(pos) = req.find(&search) {
        let after = &req[pos + search.len()..];
        let raw = after.split(|c| c == '&' || c == ' ' || c == '\r' || c == '\n').next().unwrap_or("");
        let decoded = urlencoding_decode(raw);
        if !decoded.trim().is_empty() {
            return Some(decoded.trim().to_string());
        }
    }
    None
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
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

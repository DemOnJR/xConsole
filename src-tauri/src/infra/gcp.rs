//! Read-only GCP Storage API calls for agent context before planning.

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::secrets;
use crate::storage::models::CloudAccount;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const STORAGE_SCOPE: &str = "https://www.googleapis.com/auth/devstorage.read_only";

#[derive(Debug, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    project_id: Option<String>,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct BucketList {
    items: Option<Vec<BucketItem>>,
}

#[derive(Debug, Deserialize)]
struct BucketItem {
    name: String,
    location: Option<String>,
}

fn load_service_account(account: &CloudAccount) -> Result<ServiceAccount, String> {
    let key = secrets::cloud_account_key(&account.id);
    let secret = secrets::get_secret(&key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "GCP credentials missing from keychain".to_string())?;
    serde_json::from_str(secret.trim()).map_err(|e| format!("invalid GCP service account JSON: {e}"))
}

async fn access_token(sa: &ServiceAccount) -> Result<String, String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        iss: &sa.client_email,
        scope: STORAGE_SCOPE,
        aud: TOKEN_URL,
        iat: now,
        exp: now + 3600,
    };
    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
        .map_err(|e| format!("invalid GCP private key: {e}"))?;
    let jwt = encode(&header, &claims, &key).map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "GCP token exchange {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let body: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.access_token)
}

pub async fn list_gcs_buckets(account: &CloudAccount) -> Result<Vec<String>, String> {
    let sa = load_service_account(account)?;
    let project = account
        .project_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(sa.project_id.as_deref())
        .ok_or_else(|| "GCP account missing project_id".to_string())?;
    let token = access_token(&sa).await?;
    let url = format!("https://storage.googleapis.com/storage/v1/b?project={project}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "GCS list {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let body: BucketList = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body
        .items
        .unwrap_or_default()
        .into_iter()
        .map(|b| {
            let loc = b.location.unwrap_or_else(|| "?".into());
            format!("{} ({loc})", b.name)
        })
        .collect())
}

pub async fn list_resources(account: &CloudAccount, resource: &str) -> Result<String, String> {
    match resource {
        "gcs_buckets" | "gcs" | "all" => {
            let buckets = list_gcs_buckets(account).await?;
            if buckets.is_empty() {
                Ok("no GCS buckets".into())
            } else {
                Ok(format!("GCS buckets ({}):\n{}", buckets.len(), buckets.join("\n")))
            }
        }
        other => Err(format!("unknown GCP resource '{other}' (use gcs_buckets or all)")),
    }
}

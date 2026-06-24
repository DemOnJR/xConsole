//! Cloud credential env snippets for Terraform runs on a VPS runner.

use std::collections::HashMap;

use crate::ai::AgentHome;
use crate::secrets;
use crate::ssh::shell_quote;
use crate::storage::models::CloudAccount;
use crate::storage::Db;

/// Parse AWS secret: `key_id\nsecret` or JSON with access_key_id/secret_access_key.
pub(crate) fn parse_aws_secret_for_api(raw: &str) -> Result<(String, String), String> {
    parse_aws_secret(raw)
}

fn parse_aws_secret(raw: &str) -> Result<(String, String), String> {
    let raw = raw.trim();
    if raw.starts_with('{') {
        let v: serde_json::Value =
            serde_json::from_str(raw).map_err(|e| format!("invalid AWS secret JSON: {e}"))?;
        let id = v
            .get("access_key_id")
            .or_else(|| v.get("AWS_ACCESS_KEY_ID"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| "AWS secret JSON missing access_key_id".to_string())?;
        let secret = v
            .get("secret_access_key")
            .or_else(|| v.get("AWS_SECRET_ACCESS_KEY"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| "AWS secret JSON missing secret_access_key".to_string())?;
        return Ok((id.to_string(), secret.to_string()));
    }
    let mut lines = raw.lines();
    let id = lines
        .next()
        .ok_or_else(|| "AWS secret must be access_key_id then secret_access_key (two lines)".to_string())?;
    let secret = lines
        .next()
        .ok_or_else(|| "AWS secret missing second line (secret_access_key)".to_string())?;
    Ok((id.to_string(), secret.to_string()))
}

/// Shell exports for AWS credentials. Never logged; injected only at run time.
pub fn aws_env(account: &CloudAccount, secret: &str) -> Result<String, String> {
    let (key_id, key_secret) = parse_aws_secret(secret)?;
    let region = account
        .region
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("us-east-1");
    Ok(format!(
        "export AWS_ACCESS_KEY_ID={} AWS_SECRET_ACCESS_KEY={} AWS_DEFAULT_REGION={}",
        shell_quote(&key_id),
        shell_quote(&key_secret),
        shell_quote(region),
    ))
}

/// Write GCP service-account JSON to a temp file on the runner and export GOOGLE_APPLICATION_CREDENTIALS.
pub fn gcp_env(account_id: &str, secret: &str) -> Result<String, String> {
    let path = format!("$HOME/.xconsole-gcp-{account_id}.json");
    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        secret.trim().as_bytes(),
    );
    Ok(format!(
        "printf %s {} > {path} && chmod 600 {path} && export GOOGLE_APPLICATION_CREDENTIALS={path}",
        shell_quote(&b64),
        path = path,
    ))
}

/// TFC / Terraform Cloud API token for remote backend auth.
pub fn tfc_env(secret: &str) -> String {
    format!(
        "export TF_TOKEN_app_terraform_io={}",
        shell_quote(secret.trim())
    )
}

/// Build a shell credential prefix for a linked cloud account.
pub fn credential_prefix(db: &Db, account_id: &str) -> Result<Option<String>, String> {
    let account = db
        .get_cloud_account(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("cloud account '{account_id}' not found"))?;
    if !account.has_secret {
        return Err(format!(
            "cloud account '{}' has no credentials in the keychain",
            account.name
        ));
    }
    let key = secrets::cloud_account_key(&account.id);
    let secret = secrets::get_secret(&key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "cloud credentials missing from keychain".to_string())?;
    let raw = secret.to_string();
    let snippet = match account.kind.as_str() {
        "aws" => aws_env(&account, &raw)?,
        "gcp" => gcp_env(&account.id, &raw)?,
        "tfc" => tfc_env(&raw),
        other => return Err(format!("unsupported cloud kind '{other}'")),
    };
    Ok(Some(snippet))
}

/// Process environment variables for local Terraform runs (no shell exports).
pub fn credential_env_map(
    db: &Db,
    home: &AgentHome,
    account_id: &str,
) -> Result<Option<HashMap<String, String>>, String> {
    let account = db
        .get_cloud_account(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("cloud account '{account_id}' not found"))?;
    if !account.has_secret {
        return Err(format!(
            "cloud account '{}' has no credentials in the keychain",
            account.name
        ));
    }
    let key = secrets::cloud_account_key(&account.id);
    let secret = secrets::get_secret(&key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "cloud credentials missing from keychain".to_string())?;
    let raw = secret.to_string();
    let mut env = HashMap::new();
    match account.kind.as_str() {
        "aws" => {
            let (key_id, key_secret) = parse_aws_secret(&raw)?;
            let region = account
                .region
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("us-east-1");
            env.insert("AWS_ACCESS_KEY_ID".into(), key_id);
            env.insert("AWS_SECRET_ACCESS_KEY".into(), key_secret);
            env.insert("AWS_DEFAULT_REGION".into(), region.to_string());
        }
        "gcp" => {
            let path = crate::infra::terraform_local::write_gcp_cred_file(home, &account.id, &raw)?;
            env.insert(
                "GOOGLE_APPLICATION_CREDENTIALS".into(),
                path.to_string_lossy().into_owned(),
            );
        }
        "tfc" => {
            env.insert(
                "TF_TOKEN_app_terraform_io".into(),
                raw.trim().to_string(),
            );
        }
        other => return Err(format!("unsupported cloud kind '{other}'")),
    }
    Ok(Some(env))
}

pub fn format_account_list(accounts: &[CloudAccount]) -> String {
    if accounts.is_empty() {
        return "no cloud accounts configured".into();
    }
    accounts
        .iter()
        .map(|a| {
            let creds = if a.has_secret { "credentials: set" } else { "credentials: missing" };
            let extra = match a.kind.as_str() {
                "aws" => a.region.as_deref().unwrap_or("us-east-1"),
                "gcp" => a.project_id.as_deref().unwrap_or("-"),
                "tfc" => a.organization.as_deref().unwrap_or("-"),
                _ => "-",
            };
            format!(
                "{} (id: {}, kind: {}, {}, config: {})",
                a.name, a.id, a.kind, creds, extra
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aws_two_line_secret() {
        let (id, sec) = parse_aws_secret("AKIAEXAMPLE\nsecret123").unwrap();
        assert_eq!(id, "AKIAEXAMPLE");
        assert_eq!(sec, "secret123");
    }
}

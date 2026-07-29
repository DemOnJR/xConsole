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

/// Export GCP service-account JSON for a run on a VPS runner.
///
/// The JSON goes inline in `GOOGLE_CREDENTIALS`, which the Terraform Google provider
/// accepts as either a path or the file's contents. No file is written.
///
/// What this replaces was both leaky and broken. It wrote
/// `$HOME/.xconsole-gcp-<id>.json` and pointed `GOOGLE_APPLICATION_CREDENTIALS` at it,
/// but the bytes it wrote were **base64** of the JSON, which that variable does not
/// accept — so GCP runs could not have been working. And the file was created at the
/// default umask before `chmod 600` (briefly world-readable), then left on the server
/// forever, so a service-account private key accumulated in the home directory of every
/// host used as a runner.
pub fn gcp_env(account_id: &str, secret: &str) -> Result<String, String> {
    let export = format!("export GOOGLE_CREDENTIALS={}", shell_quote(secret.trim()));

    // Reap the key file older builds left behind. Guarded on the id's shape so this can
    // never widen into an arbitrary `rm`: ids are UUIDs, and `$HOME` has to stay outside
    // the quoting to expand, so the interpolated part must be known-safe.
    let safe_id =
        !account_id.is_empty() && account_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if safe_id {
        return Ok(format!(
            "rm -f \"$HOME/.xconsole-gcp-{account_id}.json\"; {export}"
        ));
    }
    Ok(export)
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
            // Pass the service-account JSON inline. The Terraform Google provider accepts
            // either a path or the file's contents in GOOGLE_CREDENTIALS, so there is no
            // reason to materialise it.
            //
            // Previously this wrote the JSON — which contains an RSA private key — to
            // `<app_data>/agent/.cloud-creds/gcp-<id>.json` at the default umask, and
            // nothing ever deleted it. Copying the app data folder was enough to walk away
            // with a working GCP service account: no keychain, no master password, not even
            // a running app. An env var is visible to the same user via /proc, but it dies
            // with the process instead of persisting indefinitely.
            env.insert("GOOGLE_CREDENTIALS".into(), raw.trim().to_string());
            // Reap a key an earlier build may already have left behind.
            crate::infra::terraform_local::remove_legacy_gcp_cred_file(home, &account.id);
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

    #[test]
    fn gcp_env_writes_no_file_and_passes_json_inline() {
        let json = r#"{"type":"service_account","private_key":"-----BEGIN PRIVATE KEY-----\nabc\n"}"#;
        let out = gcp_env("11111111-2222-3333-4444-555555555555", json).unwrap();
        // The whole point: nothing lands on disk.
        assert!(!out.contains("printf"), "{out}");
        assert!(!out.contains("chmod"), "{out}");
        assert!(!out.contains("GOOGLE_APPLICATION_CREDENTIALS"), "{out}");
        assert!(out.contains("export GOOGLE_CREDENTIALS='"), "{out}");
        // Raw JSON, not base64 — the old code wrote base64 into a variable that only
        // accepts a path or JSON, so it cannot have worked.
        assert!(out.contains("service_account"), "{out}");
        // And it cleans up what older builds left on the runner.
        assert!(
            out.contains(r#"rm -f "$HOME/.xconsole-gcp-11111111-2222-3333-4444-555555555555.json""#),
            "{out}"
        );
    }

    #[test]
    fn gcp_env_refuses_to_build_an_rm_from_an_odd_id() {
        // Defensive: an id that isn't UUID-shaped must not reach the `rm`, since $HOME
        // has to sit outside the quoting in order to expand.
        let out = gcp_env("../../etc; rm -rf /", "{}").unwrap();
        assert!(!out.contains("rm -f"), "{out}");
        assert!(out.starts_with("export GOOGLE_CREDENTIALS="), "{out}");
    }

    #[test]
    fn gcp_env_quotes_a_json_payload_containing_quotes() {
        let out = gcp_env("abc", r#"{"a":"it's"}"#).unwrap();
        // POSIX close-escape-reopen keeps the payload as one argument. Note the
        // surrounding characters here are the JSON's double quotes, not shell quotes,
        // so match only the escape itself.
        assert!(out.contains(r#"it'\''s"#), "{out}");
        // The whole value is still wrapped in one pair of shell quotes.
        assert!(out.contains(r#"GOOGLE_CREDENTIALS='{"a":"it'\''s"}'"#), "{out}");
    }
}

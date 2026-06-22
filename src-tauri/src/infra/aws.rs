//! Read-only AWS API calls (SigV4) for agent context before planning.

use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::Method;
use sha2::{Digest, Sha256};

use crate::infra::cloud;
use crate::secrets;
use crate::storage::models::CloudAccount;

type HmacSha256 = Hmac<Sha256>;

struct AwsCreds {
    access_key: String,
    secret_key: String,
    region: String,
}

fn load_aws_creds(account: &CloudAccount) -> Result<AwsCreds, String> {
    let key = secrets::cloud_account_key(&account.id);
    let secret = secrets::get_secret(&key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "AWS credentials missing from keychain".to_string())?;
    let (access_key, secret_key) = cloud::parse_aws_secret_for_api(&secret)?;
    let region = account
        .region
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("us-east-1")
        .to_string();
    Ok(AwsCreds {
        access_key,
        secret_key,
        region,
    })
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn sign_request(
    creds: &AwsCreds,
    method: Method,
    host: &str,
    path: &str,
    query: &str,
    payload: &str,
    service: &str,
) -> Result<(String, String), String> {
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let canonical_uri = path;
    let canonical_query = query;
    let canonical_headers = format!("host:{host}\nx-amz-date:{amz_date}\n");
    let signed_headers = "host;x-amz-date";
    let payload_hash = sha256_hex(payload.as_bytes());
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        method = method.as_str(),
    );
    let credential_scope = format!("{date_stamp}/{}/{service}/aws4_request", creds.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let sig_key = signing_key(&creds.secret_key, &date_stamp, &creds.region, service);
    let signature = hex::encode(hmac_sha256(&sig_key, string_to_sign.as_bytes()));
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        creds.access_key, credential_scope, signed_headers, signature
    );
    Ok((auth, amz_date))
}

async fn signed_get(
    creds: &AwsCreds,
    host: &str,
    path: &str,
    service: &str,
) -> Result<String, String> {
    let (auth, amz_date) = sign_request(creds, Method::GET, host, path, "", "", service)?;
    let url = format!("https://{host}{path}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", auth)
        .header("x-amz-date", amz_date)
        .header("Host", host)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("AWS {service} {}: {body}", status));
    }
    Ok(body)
}

async fn signed_post_form(
    creds: &AwsCreds,
    host: &str,
    path: &str,
    form: &str,
    service: &str,
) -> Result<String, String> {
    let (auth, amz_date) = sign_request(creds, Method::POST, host, path, "", form, service)?;
    let url = format!("https://{host}{path}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", auth)
        .header("x-amz-date", amz_date)
        .header("Host", host)
        .header("Content-Type", "application/x-www-form-urlencoded; charset=utf-8")
        .body(form.to_string())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("AWS {service} {}: {body}", status));
    }
    Ok(body)
}

pub async fn list_s3_buckets(account: &CloudAccount) -> Result<Vec<String>, String> {
    let creds = load_aws_creds(account)?;
    let body = signed_get(&creds, "s3.amazonaws.com", "/", "s3").await?;
    let mut names = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("<Name>") && t.ends_with("</Name>") {
            names.push(t.trim_start_matches("<Name>").trim_end_matches("</Name>").to_string());
        }
    }
    Ok(names)
}

pub async fn list_ec2_instances(account: &CloudAccount) -> Result<Vec<String>, String> {
    let creds = load_aws_creds(account)?;
    let host = format!("ec2.{}.amazonaws.com", creds.region);
    let form = "Action=DescribeInstances&Version=2016-11-15";
    let body = signed_post_form(&creds, &host, "/", form, "ec2").await?;
    let mut out = Vec::new();
    let mut instance_id = String::new();
    let mut state = String::new();
    let mut name = String::new();
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("<instanceId>") {
            instance_id = t
                .trim_start_matches("<instanceId>")
                .trim_end_matches("</instanceId>")
                .to_string();
        } else if t.starts_with("<name>") && t.ends_with("</name>") && name.is_empty() {
            // state name tag appears before other names in some responses
            let val = t.trim_start_matches("<name>").trim_end_matches("</name>");
            if val == "running" || val == "stopped" || val == "pending" || val == "terminated" {
                state = val.to_string();
            }
        } else if t.starts_with("<value>") && !instance_id.is_empty() && name.is_empty() {
            let val = t.trim_start_matches("<value>").trim_end_matches("</value>");
            if val != instance_id && state != val {
                name = val.to_string();
            }
        } else if t.starts_with("</item>") && !instance_id.is_empty() {
            out.push(format!(
                "{instance_id} state={} name={}",
                if state.is_empty() { "?" } else { &state },
                if name.is_empty() { "-" } else { &name }
            ));
            instance_id.clear();
            state.clear();
            name.clear();
        }
    }
    Ok(out)
}

pub async fn list_resources(account: &CloudAccount, resource: &str) -> Result<String, String> {
    match resource {
        "s3_buckets" | "s3" => {
            let buckets = list_s3_buckets(account).await?;
            if buckets.is_empty() {
                Ok("no S3 buckets".into())
            } else {
                Ok(format!("S3 buckets ({}):\n{}", buckets.len(), buckets.join("\n")))
            }
        }
        "ec2_instances" | "ec2" => {
            let instances = list_ec2_instances(account).await?;
            if instances.is_empty() {
                Ok("no EC2 instances".into())
            } else {
                Ok(format!("EC2 instances ({}):\n{}", instances.len(), instances.join("\n")))
            }
        }
        "all" => {
            let buckets = list_s3_buckets(account).await.unwrap_or_else(|e| vec![format!("error: {e}")]);
            let instances = list_ec2_instances(account).await.unwrap_or_else(|e| vec![format!("error: {e}")]);
            Ok(format!(
                "S3 ({}):\n{}\n\nEC2 ({}):\n{}",
                buckets.len(),
                buckets.join("\n"),
                instances.len(),
                instances.join("\n")
            ))
        }
        other => Err(format!("unknown AWS resource '{other}' (use s3_buckets, ec2_instances, or all)")),
    }
}

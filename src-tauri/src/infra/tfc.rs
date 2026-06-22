//! Terraform Cloud API: list workspaces, upload config, trigger runs.

use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Deserialize;
use serde_json::json;

use crate::ai::AgentHome;
use crate::infra::projects::{list_project_files, read_project_file, slugify};
use crate::secrets;
use crate::storage::models::{CloudAccount, InfraProject};

const TFC_API: &str = "https://app.terraform.io/api/v2";

#[derive(Debug, Deserialize)]
struct TfcListResponse {
    data: Vec<TfcWorkspace>,
}

#[derive(Debug, Deserialize)]
struct TfcWorkspace {
    id: String,
    attributes: TfcWorkspaceAttrs,
}

#[derive(Debug, Deserialize)]
struct TfcWorkspaceAttrs {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TfcSingleResponse {
    data: TfcWorkspace,
}

#[derive(Debug, Deserialize)]
struct TfcConfigVersionResponse {
    data: TfcConfigVersion,
}

#[derive(Debug, Deserialize)]
struct TfcConfigVersion {
    id: String,
    attributes: TfcConfigVersionAttrs,
}

#[derive(Debug, Deserialize)]
struct TfcConfigVersionAttrs {
    #[serde(rename = "upload-url")]
    upload_url: String,
}

#[derive(Debug, Deserialize)]
struct TfcRunResponse {
    data: TfcRun,
}

#[derive(Debug, Deserialize)]
struct TfcRun {
    id: String,
    attributes: TfcRunAttrs,
}

#[derive(Debug, Deserialize)]
struct TfcRunAttrs {
    status: String,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TfcPlanResponse {
    data: TfcPlan,
}

#[derive(Debug, Deserialize)]
struct TfcPlan {
    attributes: TfcPlanAttrs,
}

#[derive(Debug, Deserialize)]
struct TfcPlanAttrs {
    status: String,
    #[serde(rename = "has-changes")]
    has_changes: Option<bool>,
    #[serde(rename = "resource-additions")]
    resource_additions: Option<i64>,
    #[serde(rename = "resource-changes")]
    resource_changes: Option<i64>,
    #[serde(rename = "resource-destructions")]
    resource_destructions: Option<i64>,
}

fn api_client(token: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            h.insert(
                reqwest::header::CONTENT_TYPE,
                "application/vnd.api+json".parse().unwrap(),
            );
            h
        })
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// List workspace names in a TFC organization.
pub async fn list_workspaces(account: &CloudAccount, token: &str) -> Result<Vec<String>, String> {
    let org = account
        .organization
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "TFC account missing organization".to_string())?;
    let url = format!("{TFC_API}/organizations/{org}/workspaces");
    let resp = api_client(token)
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "TFC API {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let body: TfcListResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body
        .data
        .into_iter()
        .map(|w| w.attributes.name)
        .collect())
}

pub fn load_tfc_token(account_id: &str) -> Result<String, String> {
    secrets::get_secret(&secrets::cloud_account_key(account_id))
        .map_err(|e| e.to_string())?
        .map(|s| s.to_string())
        .ok_or_else(|| "TFC token not in keychain".to_string())
}

pub fn project_tfc_config(project: &InfraProject) -> Result<(String, String), String> {
    let cfg = crate::infra::target::parse_config_json(&project.config_json);
    let org = crate::infra::target::config_str(&cfg, "tfc_org", "");
    if org.is_empty() {
        return Err("project config_json missing tfc_org".into());
    }
    let ws = crate::infra::target::config_str(&cfg, "tfc_workspace", &project.slug);
    Ok((org, ws))
}

pub async fn get_workspace_id(org: &str, workspace: &str, token: &str) -> Result<String, String> {
    let url = format!("{TFC_API}/organizations/{org}/workspaces/{workspace}");
    let resp = api_client(token)
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "TFC workspace lookup {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let body: TfcSingleResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.data.id)
}

/// Gzip tarball of project `.tf` files (excludes `.terraform/`).
pub fn build_config_tarball(home: &AgentHome, slug: &str) -> Result<Vec<u8>, String> {
    let slug = slugify(slug);
    let files = list_project_files(home, &slug)?;
    if files.is_empty() {
        return Err("project has no files to upload".into());
    }
    let mut gz_buf = Vec::new();
    {
        let enc = GzEncoder::new(&mut gz_buf, Compression::default());
        let mut tar = tar::Builder::new(enc);
        for rel in files {
            let content = read_project_file(home, &slug, &rel)?;
            let mut header = tar::Header::new_gnu();
            header.set_path(&rel).map_err(|e| e.to_string())?;
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, content.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let enc = tar.into_inner().map_err(|e| e.to_string())?;
        enc.finish().map_err(|e| e.to_string())?;
    }
    Ok(gz_buf)
}

pub async fn upload_configuration(
    workspace_id: &str,
    tarball: &[u8],
    token: &str,
) -> Result<String, String> {
    let url = format!("{TFC_API}/workspaces/{workspace_id}/configuration-versions");
    let body = json!({
        "data": {
            "type": "configuration-versions",
            "attributes": {
                "auto-queue-runs": false
            }
        }
    });
    let resp = api_client(token)
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "TFC create config version {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let cv: TfcConfigVersionResponse = resp.json().await.map_err(|e| e.to_string())?;
    let upload_url = cv.data.attributes.upload_url;
    let cv_id = cv.data.id;

    let upload = reqwest::Client::new()
        .put(&upload_url)
        .header("Content-Type", "application/octet-stream")
        .body(tarball.to_vec())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !upload.status().is_success() {
        return Err(format!(
            "TFC config upload {}: {}",
            upload.status(),
            upload.text().await.unwrap_or_default()
        ));
    }
    Ok(cv_id)
}

pub async fn create_run(
    workspace_id: &str,
    config_version_id: &str,
    token: &str,
    auto_apply: bool,
    message: &str,
) -> Result<String, String> {
    let url = format!("{TFC_API}/runs");
    let body = json!({
        "data": {
            "type": "runs",
            "attributes": {
                "message": message,
                "auto-apply": auto_apply
            },
            "relationships": {
                "workspace": {
                    "data": { "type": "workspaces", "id": workspace_id }
                },
                "configuration-version": {
                    "data": { "type": "configuration-versions", "id": config_version_id }
                }
            }
        }
    });
    let resp = api_client(token)
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "TFC create run {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let run: TfcRunResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(run.data.id)
}

pub async fn get_run_status(run_id: &str, token: &str) -> Result<String, String> {
    let url = format!("{TFC_API}/runs/{run_id}");
    let resp = api_client(token)
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "TFC get run {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let run: TfcRunResponse = resp.json().await.map_err(|e| e.to_string())?;
    let msg = run
        .data
        .attributes
        .message
        .unwrap_or_else(|| "—".into());
    let mut out = format!(
        "run_id: {}\nstatus: {}\nmessage: {}\nurl: https://app.terraform.io/app/runs/{run_id}",
        run.data.id, run.data.attributes.status, msg, run_id = run.data.id
    );
    if let Some(summary) = fetch_plan_summary(run_id, token).await.ok().flatten() {
        out.push_str(&format!("\nplan: {summary}"));
    }
    Ok(out)
}

async fn fetch_plan_summary(run_id: &str, token: &str) -> Result<Option<String>, String> {
    let url = format!("{TFC_API}/runs/{run_id}/plan");
    let resp = api_client(token)
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!(
            "TFC get plan {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ));
    }
    let plan: TfcPlanResponse = resp.json().await.map_err(|e| e.to_string())?;
    let a = &plan.data.attributes;
    Ok(Some(format!(
        "status={} add={} change={} destroy={} has_changes={}",
        a.status,
        a.resource_additions.unwrap_or(0),
        a.resource_changes.unwrap_or(0),
        a.resource_destructions.unwrap_or(0),
        a.has_changes.unwrap_or(false)
    )))
}

/// Upload project config and queue a plan or apply run on TFC.
pub async fn trigger_run(
    home: &AgentHome,
    project: &InfraProject,
    _account: &CloudAccount,
    token: &str,
    apply: bool,
) -> Result<String, String> {
    let (org, ws) = project_tfc_config(project)?;
    let workspace_id = get_workspace_id(&org, &ws, token).await?;
    let tarball = build_config_tarball(home, &project.slug)?;
    let cv_id = upload_configuration(&workspace_id, &tarball, token).await?;
    let action = if apply { "apply" } else { "plan" };
    let run_id = create_run(
        &workspace_id,
        &cv_id,
        token,
        apply,
        &format!("xConsole {action} for {}", project.slug),
    )
    .await?;
    Ok(format!(
        "TFC {action} queued\nrun_id: {run_id}\nworkspace: {org}/{ws}\nstatus: use tfc_run_status with run_id"
    ))
}

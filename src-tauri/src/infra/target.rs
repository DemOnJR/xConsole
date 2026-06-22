//! Resolve where Terraform commands execute: local, VPS runner, or TFC remote.

use serde_json::Value;

use crate::ai::tools::{is_target_allowed, resolve_target, ToolContext};
use crate::storage::models::InfraProject;

/// Where a terraform subcommand runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerraformExecution {
    /// Run `terraform` on the agent host (desktop).
    Local,
    /// Sync project files to a VPS and run via SSH.
    Vps(String),
    /// Upload config tarball and queue a run on Terraform Cloud.
    TfcRemote,
}

pub fn parse_config_json(config_json: &Option<String>) -> Value {
    config_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn config_str(config: &Value, key: &str, default: &str) -> String {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// Pick local / VPS / TFC execution for a terraform subcommand.
pub async fn resolve_execution(
    ctx: &ToolContext,
    args: &Value,
    slug: &str,
    subcommand: &str,
) -> Result<(TerraformExecution, InfraProject), String> {
    let project = ctx
        .db
        .get_infra_project(slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project '{slug}' not found"))?;

    if let Some(runner) = args.get("runner").and_then(|v| v.as_str()) {
        return match runner {
            "local" => Ok((TerraformExecution::Local, project)),
            "tfc" if project.backend == "tfc" => Ok((TerraformExecution::TfcRemote, project)),
            "tfc" => Err("runner=tfc requires project backend=tfc".into()),
            "vps" => {
                let vps_id = resolve_vps_runner(ctx, args, &project).await?;
                Ok((TerraformExecution::Vps(vps_id), project))
            }
            other => Err(format!("unknown runner '{other}' (use local, vps, or tfc)")),
        };
    }

    if project.backend == "tfc" && matches!(subcommand, "plan" | "apply") {
        return Ok((TerraformExecution::TfcRemote, project));
    }

    let vps_explicit = args
        .get("vps_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let has_default = project
        .default_vps_id
        .as_ref()
        .filter(|s| !s.is_empty());
    if vps_explicit.is_none() && has_default.is_none() && ctx.targets.is_empty() {
        return Ok((TerraformExecution::Local, project));
    }

    let vps_id = resolve_vps_runner(ctx, args, &project).await?;
    Ok((TerraformExecution::Vps(vps_id), project))
}

async fn resolve_vps_runner(
    ctx: &ToolContext,
    args: &Value,
    project: &InfraProject,
) -> Result<String, String> {
    if let Some(id) = args.get("vps_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        if !is_target_allowed(&ctx.targets, id) {
            return Err(format!("vps_id '{id}' is not in the selected targets"));
        }
        return Ok(id.to_string());
    }
    if let Some(id) = project.default_vps_id.as_ref().filter(|s| !s.is_empty()) {
        if is_target_allowed(&ctx.targets, id) || ctx.targets.is_empty() {
            return Ok(id.clone());
        }
    }
    resolve_target(ctx, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_str_reads_json() {
        let cfg = parse_config_json(&Some(r#"{"tfc_org":"acme","aws_region":"eu-west-1"}"#.into()));
        assert_eq!(config_str(&cfg, "tfc_org", ""), "acme");
        assert_eq!(config_str(&cfg, "aws_region", "us-east-1"), "eu-west-1");
        assert_eq!(config_str(&cfg, "missing", "default"), "default");
    }
}

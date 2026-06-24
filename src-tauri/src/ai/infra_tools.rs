//! Agent tools for Terraform projects: local runner, VPS runner, TFC remote.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::ai::provider::{emit, EventSink, ToolDef};
use crate::ai::tools::ToolContext;
use crate::infra::cloud::{self, format_account_list};
use crate::infra::projects::{
    format_project_list, list_project_files, read_project_file, scaffold, slugify,
    write_project_file,
};
use crate::infra::target::{self, TerraformExecution};
use crate::infra::terraform::{
    build_remote_terraform_command, is_readonly_subcommand, run_on_vps, summarize_plan,
    vps_var_args,
};
use crate::infra::terraform_local::{describe_command, run_local};
use crate::infra::tfc;

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "project_create".into(),
            description: "Create a new Terraform project (local files + DB record). Load skill infra/terraform-vps and meta/ponytail before designing HCL. Templates: blank, vps-web, aws-minimal, gcp-minimal. Set backend=tfc with config_json for remote state.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "slug": {"type": "string"},
                    "template": {"type": "string", "enum": ["blank", "vps-web", "aws-minimal", "gcp-minimal"]},
                    "backend": {"type": "string", "enum": ["vps", "tfc"]},
                    "default_vps_id": {"type": "string"},
                    "cloud_account_id": {"type": "string", "description": "AWS/GCP/TFC credentials id"},
                    "config_json": {"type": "string", "description": "JSON: aws_region, gcp_region, tfc_org, tfc_workspace"},
                    "description": {"type": "string"}
                },
                "required": ["name"]
            }),
        },
        ToolDef {
            name: "cloud_account_list".into(),
            description: "List configured cloud accounts (AWS, GCP, Terraform Cloud). No secrets returned.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "cloud_list_resources".into(),
            description: "Read-only cloud inventory before planning. AWS: s3_buckets, ec2_instances, all. GCP: gcs_buckets, all.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string"},
                    "resource": {"type": "string", "description": "AWS: s3_buckets|ec2_instances|all. GCP: gcs_buckets|all."}
                },
                "required": ["cloud_account_id"]
            }),
        },
        ToolDef {
            name: "tfc_list_workspaces".into(),
            description: "List Terraform Cloud workspaces for a TFC cloud account.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string"}
                },
                "required": ["cloud_account_id"]
            }),
        },
        ToolDef {
            name: "tfc_run_status".into(),
            description: "Poll status of a Terraform Cloud run by run_id.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string"},
                    "run_id": {"type": "string"}
                },
                "required": ["cloud_account_id", "run_id"]
            }),
        },
        ToolDef {
            name: "project_list".into(),
            description: "List all Terraform projects.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "project_read".into(),
            description: "Read a file from a project (e.g. main.tf). Omit path to list files.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                    "path": {"type": "string"}
                },
                "required": ["slug"]
            }),
        },
        ToolDef {
            name: "project_write".into(),
            description: "Write or overwrite a file in a project.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["slug", "path", "content"]
            }),
        },
        ToolDef {
            name: "terraform_init".into(),
            description: "Run terraform init. Uses local runner when no VPS is selected; TFC backend projects queue remote runs for plan/apply. Optional runner: local|vps|tfc.".into(),
            parameters: terraform_params(false),
        },
        ToolDef {
            name: "terraform_plan".into(),
            description: "Run terraform plan. backend=tfc projects queue a TFC plan run (no VPS). Otherwise local or VPS runner.".into(),
            parameters: terraform_params(false),
        },
        ToolDef {
            name: "terraform_apply".into(),
            description: "Run terraform apply. backend=tfc projects queue a TFC apply run. Requires approval unless safety=full.".into(),
            parameters: terraform_params(true),
        },
    ]
}

fn terraform_params(apply: bool) -> Value {
    let extra_desc = if apply {
        "Pass -auto-approve only when the user explicitly asked."
    } else {
        "Extra terraform plan/init flags."
    };
    json!({
        "type": "object",
        "properties": {
            "slug": {"type": "string"},
            "vps_id": {"type": "string"},
            "runner": {"type": "string", "enum": ["local", "vps", "tfc"], "description": "Override execution target"},
            "extra_args": {"type": "string", "description": extra_desc}
        },
        "required": ["slug"]
    })
}

pub async fn dispatch(ctx: &ToolContext, name: &str, args: &Value, sink: &EventSink) -> String {
    match name {
        "project_create" => project_create(ctx, args).await,
        "project_list" => project_list(ctx).await,
        "cloud_account_list" => cloud_account_list(ctx).await,
        "cloud_list_resources" => cloud_list_resources(ctx, args).await,
        "tfc_list_workspaces" => tfc_list_workspaces(ctx, args).await,
        "tfc_run_status" => tfc_run_status(ctx, args).await,
        "project_read" => project_read(ctx, args).await,
        "project_write" => project_write(ctx, args).await,
        "terraform_init" => terraform_run(ctx, args, sink, "init", "").await,
        "terraform_plan" => {
            let extra = args
                .get("extra_args")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            terraform_run(ctx, args, sink, "plan", extra).await
        }
        "terraform_apply" => {
            let extra = args
                .get("extra_args")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            terraform_run(ctx, args, sink, "apply", extra).await
        }
        other => format!("error: unknown infra tool '{other}'"),
    }
}

async fn project_create(ctx: &ToolContext, args: &Value) -> String {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n,
        _ => return "error: missing 'name'".into(),
    };
    let input = crate::storage::models::InfraProjectInput {
        id: None,
        name: name.to_string(),
        slug: args.get("slug").and_then(|v| v.as_str()).map(String::from),
        template: args
            .get("template")
            .and_then(|v| v.as_str())
            .map(String::from),
        backend: args.get("backend").and_then(|v| v.as_str()).map(String::from),
        default_vps_id: args
            .get("default_vps_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        cloud_account_id: args
            .get("cloud_account_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        config_json: args
            .get("config_json")
            .and_then(|v| v.as_str())
            .map(String::from),
        description: args
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    // Resolve the slug and refuse before touching disk if it already exists, so a
    // name collision can't clobber another project's .tf files.
    let slug = match crate::infra::projects::project_slug(&input) {
        Ok(s) => s,
        Err(e) => return format!("error: {e}"),
    };
    match ctx.db.get_infra_project(&slug) {
        Ok(Some(p)) => {
            return format!(
                "error: a project with slug '{}' already exists (id {}); use project_write or terraform_* to work with it, or choose a different name",
                p.slug, p.id
            )
        }
        Ok(None) => {}
        Err(e) => return format!("error: {e}"),
    }
    let slug = match scaffold(&ctx.home, &input) {
        Ok(s) => s,
        Err(e) => return format!("error: {e}"),
    };
    match ctx.db.upsert_infra_project(&input, &slug) {
        Ok(p) => format!("created project '{}' (slug: {})", p.name, p.slug),
        Err(e) => format!("error: {e}"),
    }
}

async fn project_list(ctx: &ToolContext) -> String {
    match ctx.db.list_infra_projects() {
        Ok(list) => format_project_list(&list),
        Err(e) => format!("error: {e}"),
    }
}

async fn project_read(ctx: &ToolContext, args: &Value) -> String {
    let slug = match args.get("slug").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => slugify(s),
        _ => return "error: missing 'slug'".into(),
    };
    if let Some(path) = args.get("path").and_then(|v| v.as_str()).filter(|p| !p.is_empty()) {
        return match read_project_file(&ctx.home, &slug, path) {
            Ok(body) => body,
            Err(e) => format!("error: {e}"),
        };
    }
    match list_project_files(&ctx.home, &slug) {
        Ok(files) if files.is_empty() => "no files in project".into(),
        Ok(files) => files.join("\n"),
        Err(e) => format!("error: {e}"),
    }
}

async fn project_write(ctx: &ToolContext, args: &Value) -> String {
    let slug = match args.get("slug").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => slugify(s),
        _ => return "error: missing 'slug'".into(),
    };
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return "error: missing 'path'".into(),
    };
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    match write_project_file(&ctx.home, &slug, path, content) {
        Ok(()) => format!("wrote {path}"),
        Err(e) => format!("error: {e}"),
    }
}

async fn cloud_account_list(ctx: &ToolContext) -> String {
    match ctx.db.list_cloud_accounts() {
        Ok(list) => format_account_list(&list),
        Err(e) => format!("error: {e}"),
    }
}

async fn cloud_list_resources(ctx: &ToolContext, args: &Value) -> String {
    let id = match args.get("cloud_account_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'cloud_account_id'".into(),
    };
    let resource = args
        .get("resource")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let account = match ctx.db.get_cloud_account(id) {
        Ok(Some(a)) => a,
        Ok(None) => return format!("error: cloud account '{id}' not found"),
        Err(e) => return format!("error: {e}"),
    };
    match account.kind.as_str() {
        "aws" => match crate::infra::aws::list_resources(&account, resource).await {
            Ok(s) => s,
            Err(e) => format!("error: {e}"),
        },
        "gcp" => match crate::infra::gcp::list_resources(&account, resource).await {
            Ok(s) => s,
            Err(e) => format!("error: {e}"),
        },
        other => format!("error: cloud_list_resources not supported for kind '{other}'"),
    }
}

async fn tfc_list_workspaces(ctx: &ToolContext, args: &Value) -> String {
    let id = match args.get("cloud_account_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'cloud_account_id'".into(),
    };
    let account = match ctx.db.get_cloud_account(id) {
        Ok(Some(a)) => a,
        Ok(None) => return format!("error: cloud account '{id}' not found"),
        Err(e) => return format!("error: {e}"),
    };
    if account.kind != "tfc" {
        return "error: account is not kind 'tfc'".into();
    }
    let token = match tfc::load_tfc_token(&account.id) {
        Ok(t) => t,
        Err(e) => return format!("error: {e}"),
    };
    match tfc::list_workspaces(&account, &token).await {
        Ok(names) if names.is_empty() => "no workspaces".into(),
        Ok(names) => names.join("\n"),
        Err(e) => format!("error: {e}"),
    }
}

async fn tfc_run_status(ctx: &ToolContext, args: &Value) -> String {
    let account_id = match args.get("cloud_account_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'cloud_account_id'".into(),
    };
    let run_id = match args.get("run_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'run_id'".into(),
    };
    // Verify the account exists and is a TFC account before loading its secret,
    // so a non-tfc account id can't trigger a cross-service keychain read.
    match ctx.db.get_cloud_account(account_id) {
        Ok(Some(acct)) if acct.kind == "tfc" => {}
        Ok(Some(_)) => return "error: cloud account is not a Terraform Cloud (tfc) account".into(),
        Ok(None) => return format!("error: cloud account '{account_id}' not found"),
        Err(e) => return format!("error: {e}"),
    }
    let token = match tfc::load_tfc_token(account_id) {
        Ok(t) => t,
        Err(e) => return format!("error: {e}"),
    };
    match tfc::get_run_status(run_id, &token).await {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    }
}

async fn project_env_map(ctx: &ToolContext, slug: &str) -> Result<HashMap<String, String>, String> {
    let project = ctx
        .db
        .get_infra_project(slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project '{slug}' not found"))?;
    let Some(account_id) = project.cloud_account_id.filter(|s| !s.is_empty()) else {
        return Ok(HashMap::new());
    };
    match cloud::credential_env_map(&ctx.db, &ctx.home, &account_id)? {
        Some(map) => Ok(map),
        None => Ok(HashMap::new()),
    }
}

async fn project_credential_prefix(ctx: &ToolContext, slug: &str) -> Result<Option<String>, String> {
    let project = ctx
        .db
        .get_infra_project(slug)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project '{slug}' not found"))?;
    let Some(account_id) = project.cloud_account_id.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    cloud::credential_prefix(&ctx.db, &account_id)
}

fn format_output(exit_code: i32, stdout: &str, stderr: &str, subcommand: &str) -> String {
    let mut s = format!("exit_code: {exit_code}\n");
    if !stdout.is_empty() {
        let body = if is_readonly_subcommand(subcommand) && subcommand == "plan" {
            summarize_plan(stdout)
        } else {
            stdout.trim_end().to_string()
        };
        s.push_str(&format!("stdout:\n{body}\n"));
    }
    if !stderr.is_empty() {
        s.push_str(&format!("stderr:\n{}\n", stderr.trim_end()));
    }
    s
}

async fn authorize_infra(
    ctx: &ToolContext,
    vps_id: Option<&str>,
    command: &str,
) -> Result<(), String> {
    let mode = match vps_id {
        Some(id) => crate::ai::safety::effective_mode(&ctx.db, &ctx.safety, id),
        None => ctx.safety.clone(),
    };
    crate::ai::safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        &mode,
        &ctx.session_id,
        vps_id,
        command,
    )
    .await
}

async fn terraform_run(
    ctx: &ToolContext,
    args: &Value,
    sink: &EventSink,
    subcommand: &str,
    extra_args: &str,
) -> String {
    let slug = match args.get("slug").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => slugify(s),
        _ => return "error: missing 'slug'".into(),
    };

    let (execution, project) = match target::resolve_execution(ctx, args, &slug, subcommand).await {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };

    match execution {
        TerraformExecution::TfcRemote => {
            if subcommand == "init" {
                return terraform_run_local(ctx, sink, &slug, subcommand, extra_args).await;
            }
            terraform_run_tfc(ctx, sink, &project, subcommand).await
        }
        TerraformExecution::Local => {
            terraform_run_local(ctx, sink, &slug, subcommand, extra_args).await
        }
        TerraformExecution::Vps(vps_id) => {
            terraform_run_vps(ctx, sink, &slug, &vps_id, subcommand, extra_args).await
        }
    }
}

async fn terraform_run_local(
    ctx: &ToolContext,
    sink: &EventSink,
    slug: &str,
    subcommand: &str,
    extra_args: &str,
) -> String {
    let mut tokens: Vec<String> = extra_args.split_whitespace().map(String::from).collect();
    if let Ok(Some(project)) = ctx.db.get_infra_project(slug) {
        if project.template == "vps-web" {
            if let Some(vps_id) = project.default_vps_id.as_deref().filter(|s| !s.is_empty()) {
                if let Ok(vars) = vps_var_args(&ctx.db, vps_id).await {
                    if !tokens.iter().any(|t| t.contains("vps_host")) {
                        tokens.extend(vars);
                    }
                }
            }
        }
    }

    let env = match project_env_map(ctx, slug).await {
        Ok(e) => e,
        Err(e) => return format!("error: {e}"),
    };

    let command = describe_command(slug, subcommand, &tokens);
    if let Err(e) = authorize_infra(ctx, None, &command).await {
        return format!("error: {e}");
    }

    emit(
        Some(sink),
        crate::ai::provider::StreamEvent::Status(format!("$ {command}")),
    );

    match run_local(&ctx.home, slug, subcommand, &tokens, &env).await {
        Ok(out) => format_output(out.exit_code, &out.stdout, &out.stderr, subcommand),
        Err(e) => format!("error running terraform: {e}"),
    }
}

async fn terraform_run_vps(
    ctx: &ToolContext,
    sink: &EventSink,
    slug: &str,
    vps_id: &str,
    subcommand: &str,
    extra_args: &str,
) -> String {
    let mut tokens: Vec<String> = extra_args.split_whitespace().map(String::from).collect();
    if let Ok(vars) = vps_var_args(&ctx.db, vps_id).await {
        if !tokens.iter().any(|t| t.contains("vps_host")) {
            tokens.extend(vars);
        }
    }

    let creds = match project_credential_prefix(ctx, slug).await {
        Ok(c) => c,
        Err(e) => return format!("error: {e}"),
    };

    let command = match build_remote_terraform_command(
        &ctx.home,
        slug,
        subcommand,
        &tokens,
        creds.as_deref(),
    ) {
        Ok(c) => c,
        Err(e) => return format!("error: {e}"),
    };

    if let Err(e) = authorize_infra(ctx, Some(vps_id), &command).await {
        return format!("error: {e}");
    }

    emit(
        Some(sink),
        crate::ai::provider::StreamEvent::Status(format!("$ terraform {subcommand} ({slug})")),
    );

    match run_on_vps(&ctx.sessions, vps_id, &command).await {
        Ok(out) => format_output(out.exit_code, &out.stdout, &out.stderr, subcommand),
        Err(e) => format!("error running terraform: {e}"),
    }
}

async fn terraform_run_tfc(
    ctx: &ToolContext,
    sink: &EventSink,
    project: &crate::storage::models::InfraProject,
    subcommand: &str,
) -> String {
    let account_id = match project.cloud_account_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => id,
        None => return "error: TFC project needs cloud_account_id (TFC token)".into(),
    };
    let account = match ctx.db.get_cloud_account(account_id) {
        Ok(Some(a)) => a,
        Ok(None) => return format!("error: cloud account '{account_id}' not found"),
        Err(e) => return format!("error: {e}"),
    };
    if account.kind != "tfc" {
        return "error: linked cloud account must be kind 'tfc'".into();
    }
    let token = match tfc::load_tfc_token(&account.id) {
        Ok(t) => t,
        Err(e) => return format!("error: {e}"),
    };

    let apply = subcommand == "apply";
    let action = if apply { "apply" } else { "plan" };
    let command = format!("TFC remote {action} for project {}", project.slug);

    if let Err(e) = authorize_infra(ctx, None, &command).await {
        return format!("error: {e}");
    }

    emit(
        Some(sink),
        crate::ai::provider::StreamEvent::Status(format!("$ {command}")),
    );

    match tfc::trigger_run(&ctx.home, project, &account, &token, apply).await {
        Ok(s) => s,
        Err(e) => format!("error: {e}"),
    }
}

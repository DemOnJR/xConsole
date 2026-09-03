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
            description: "Create a Terraform project: a directory of HCL files on this \
machine plus the record that binds it to a server or a cloud account. Nothing is deployed \
— this is where the code lives until terraform_plan and terraform_apply.\n\
Load the infra/terraform-vps and meta/ponytail skills before designing the HCL rather \
than writing it from memory. Templates: blank, vps-web, aws-minimal, gcp-minimal. Set \
backend=tfc with config_json (tfc_org, tfc_workspace from tfc_list_workspaces) to keep \
state in Terraform Cloud instead of locally.".into(),
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
            description: "List the cloud accounts connected to xConsole — AWS, GCP, \
Terraform Cloud, Cloudflare — with the id every other tool here takes as \
cloud_account_id. Call it before guessing an id. Read-only, and it never returns keys or \
tokens: credentials stay in the OS keychain and are never readable through a tool.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "cloud_list_resources".into(),
            description: "What already exists in a cloud account, read straight from the \
provider's API rather than from Terraform state. Use it before writing HCL, so a resource \
you are about to create is not one that is already there under another name — Terraform \
will happily build a second one. AWS: s3_buckets, ec2_instances, all. GCP: gcs_buckets, \
all. Read-only; it never changes anything.".into(),
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
            description: "List the Terraform Cloud workspaces on a TFC account, with the \
names project_create expects in config_json (tfc_org, tfc_workspace). Use it to bind a \
project to the right workspace instead of guessing the name — a wrong workspace name \
fails at plan time, after the HCL has already been written. Read-only.".into(),
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
            description: "Where a Terraform Cloud run has got to: queued, planning, \
needs confirmation, applying, finished or errored. TFC runs are asynchronous — \
terraform_plan and terraform_apply queue one and return a run_id immediately, and this is \
the only way to find out what happened.\n\
Do not poll it in a tight loop. A plan takes tens of seconds and an apply takes minutes: \
check once, and if it is still running do something else and check again later rather than \
calling this five times in a row. If it comes back needing confirmation, that is a person's \
decision, not yours to auto-approve — say so and stop.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string", "description": "TFC account id from cloud_account_list."},
                    "run_id": {"type": "string", "description": "The run id terraform_plan or terraform_apply returned."}
                },
                "required": ["cloud_account_id", "run_id"]
            }),
        },
        ToolDef {
            name: "cloudflare_list_zones".into(),
            description: "List the domains in the connected Cloudflare account with their \
zone ids and status. Every other Cloudflare tool takes zone_id, so this is the first call \
— a zone id guessed from a domain name is a call that fails, and a domain that is not in \
this list is not managed by Cloudflare at all. Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID in xConsole. If omitted, uses active connected account."}
                }
            }),
        },
        ToolDef {
            name: "cloudflare_get_zone_analytics".into(),
            description: "Traffic figures for a Cloudflare domain over a time window: \
requests, bandwidth, cache hit ratio and blocked threats. Use it to answer whether a \
problem is traffic-shaped — a spike, a cache that stopped working, an attack — before \
going to look at the origin server. Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "zone_id": {"type": "string", "description": "Zone ID or domain name (e.g. example.com)"},
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID"},
                    "since_minutes": {"type": "integer", "description": "Optional: time window in minutes (e.g. -1440 for last 24h, -60 for last 1h, -10080 for 7d). Default -1440."}
                },
                "required": ["zone_id"]
            }),
        },
        ToolDef {
            name: "cloudflare_list_tunnels".into(),
            description: "List the Cloudflare tunnels on an account: name, id, and whether \
each is currently connected. Use it when a site behind a tunnel is unreachable — a tunnel \
showing as down explains it, and no amount of reading the origin's config will. \
Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID in xConsole"}
                }
            }),
        },
        ToolDef {
            name: "cloudflare_list_dns".into(),
            description: "List the DNS records on one Cloudflare zone: type, name, content, \
proxy state and the record_id that cloudflare_upsert_dns needs in order to CHANGE a record \
rather than add a second one for the same name. Always read this before editing DNS. \
Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "zone_id": {"type": "string", "description": "Zone ID or domain name"},
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID"}
                },
                "required": ["zone_id"]
            }),
        },
        ToolDef {
            name: "cloudflare_upsert_dns".into(),
            description: "Create or change a DNS record on a live domain. This takes effect \
publicly within seconds — it is not a draft, and there is no confirmation step after it.\n\
Before calling it: run cloudflare_list_dns and read what is there now. Pointing an \
existing name at a new address is done by passing that record's record_id; without one \
this creates a SECOND record for the same name, and the domain then resolves to both \
hosts at random. Getting the name wrong takes a production site down, and \
\"app.example.com\" versus \"app\" is the mistake that does it — Cloudflare treats a \
bare label as a subdomain of the zone.\n\
proxied=true hides the origin IP behind Cloudflare and terminates TLS there; that is \
usually right for a website and wrong for anything that needs a direct connection (SSH, \
mail, a database). If you change something and it turns out wrong, cloudflare_get_history \
then cloudflare_revert_action undoes it.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "zone_id": {"type": "string", "description": "Zone id or domain name, from cloudflare_list_zones."},
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account id. Defaults to the connected account."},
                    "record_id": {"type": "string", "description": "The record to CHANGE, from cloudflare_list_dns. Omitting it creates a new record — so leaving it out when a record for this name already exists gives the name two answers."},
                    "type": {"type": "string", "enum": ["A", "AAAA", "CNAME", "TXT", "MX"]},
                    "name": {"type": "string", "description": "Full name, e.g. app.example.com. A bare label is treated as a subdomain of the zone."},
                    "content": {"type": "string", "description": "Target: an IPv4 for A, IPv6 for AAAA, hostname for CNAME, the text for TXT."},
                    "proxied": {"type": "boolean", "description": "Route through Cloudflare (orange cloud): hides the origin IP and terminates TLS there. Right for a website; wrong for SSH, mail or a database, which need a direct connection."}
                },
                "required": ["zone_id", "type", "name", "content"]
            }),
        },
        ToolDef {
            name: "cloudflare_set_security_level".into(),
            description: "Change how aggressively Cloudflare challenges visitors to a zone. \
This affects real traffic immediately: under_attack shows an interstitial to every \
visitor, which stops an attack and also breaks APIs, webhooks and anything non-browser \
hitting that domain. Use it for an active attack and put it back afterwards; do not raise \
it as a precaution. Levels: essentially_off, low, medium, high, under_attack.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "zone_id": {"type": "string"},
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID"},
                    "level": {"type": "string", "enum": ["essentially_off", "low", "medium", "high", "under_attack"]}
                },
                "required": ["zone_id", "level"]
            }),
        },
        ToolDef {
            name: "cloudflare_get_history".into(),
            description: "Recent changes made to a Cloudflare account through xConsole — DNS \
edits, tunnel changes, security level changes — each with the action id that \
cloudflare_revert_action takes. This is how you answer \"what did we just change?\" after \
a site breaks, and the only way to get the id needed to undo it. Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare cloud account ID"}
                }
            }),
        },
        ToolDef {
            name: "cloudflare_revert_action".into(),
            description: "Undo one Cloudflare change recorded by cloudflare_get_history, \
putting the record or setting back to what it was before. It takes effect publicly as \
soon as it runs, exactly like the change it reverses. Get the action_id from \
cloudflare_get_history — and check that the state it is reverting to is the one you \
actually want, because a revert of a revert is another live change.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action_id": {"type": "string", "description": "ID of the audit log action to revert"},
                    "cloud_account_id": {"type": "string", "description": "Optional: Cloudflare account ID"}
                },
                "required": ["action_id"]
            }),
        },
        ToolDef {
            name: "project_list".into(),
            description: "List the Terraform projects xConsole knows about: slug, template, \
backend (vps or tfc) and which server or cloud account each is bound to. Start here — \
every other project tool is addressed by slug, and guessing a slug fails the call. \
Read-only and cheap.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "project_read".into(),
            description: "Read one file out of a Terraform project — main.tf, variables.tf, \
terraform.tfvars — or, with path omitted, list what files the project has. Read before \
you write: project_write overwrites whole files, so editing something you have not read \
this session means throwing away whatever is in it. Read-only.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string", "description": "Project slug from project_list."},
                    "path": {"type": "string", "description": "File within the project, e.g. main.tf. Omit to list the project's files."}
                },
                "required": ["slug"]
            }),
        },
        ToolDef {
            name: "project_write".into(),
            description: "Write a file into a Terraform project, replacing it completely if \
it already exists — there is no partial edit here, so send the whole intended contents and \
call project_read first if you have not already seen the file this session.\n\
Writing HCL changes nothing on its own; nothing happens until terraform_plan and \
terraform_apply. That makes this the safe place to iterate — but it also means a file \
that looks right has not been checked. Always follow a change with terraform_plan and \
read the diff before applying.\n\
Load the infra/terraform-vps skill before designing resources rather than inventing HCL \
from memory.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string", "description": "Project slug from project_list."},
                    "path": {"type": "string", "description": "File within the project, e.g. main.tf."},
                    "content": {"type": "string", "description": "The complete file. This replaces whatever is there — it is not appended or merged."}
                },
                "required": ["slug", "path", "content"]
            }),
        },
        ToolDef {
            name: "terraform_init".into(),
            description: "Download providers and set up a project's backend. Run it once \
after project_create, and again whenever a provider or backend block changes — a plan \
that fails with \"provider not installed\" or \"backend changed\" is asking for this. \
Safe: it touches the working directory, never infrastructure. Runs locally when no VPS is \
selected; runner (local|vps|tfc) overrides that.".into(),
            parameters: terraform_params(false),
        },
        ToolDef {
            name: "terraform_plan".into(),
            description: "Work out what terraform_apply would change, without changing \
anything. Run this after every edit to a project's HCL and read the summary line — \
\"N to add, N to change, N to destroy\" — before deciding whether to apply. A plan that \
destroys or replaces something you did not expect is the signal to stop and ask, not to \
apply and find out.\n\
backend=tfc projects queue a run in Terraform Cloud and return a run_id, so follow with \
tfc_run_status. Changes nothing either way.".into(),
            parameters: terraform_params(false),
        },
        ToolDef {
            name: "terraform_apply".into(),
            description: "Apply a Terraform plan: this creates, changes and DESTROYS real \
infrastructure. It is the most consequential tool here, and several of the things it can \
do — dropping a database instance, releasing an address, replacing a volume — cannot be \
undone by running it again.\n\
ALWAYS run terraform_plan first and read what it says. \"Plan: 2 to add, 0 to change, 1 to \
destroy\" is the line that matters: if anything is being destroyed or replaced, say what \
and why, and get the user to agree before applying. A plan that surprises you is a reason \
to stop, not to apply and see.\n\
backend=tfc projects queue a run in Terraform Cloud and return a run_id — the apply has \
not happened yet, so follow with tfc_run_status rather than reporting it as done. Needs \
approval unless safety is set to full.".into(),
            parameters: terraform_params(true),
        },
        ToolDef {
            name: "plugin_list".into(),
            description: "List the xConsole plugins installed on this machine, whether each \
is enabled, and which agent tools each one contributes. Call it before plugin_toggle so \
you disable the plugin you meant to, and to find out why a tool you expected is not \
available. Read-only.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "plugin_install".into(),
            description: "Install an xConsole plugin from a GitHub repository \
('owner/repo' or a URL) or a local directory. A plugin runs code inside xConsole and can \
add agent tools, so install one because the user asked for that specific plugin — never \
speculatively, and never from a source they did not name. Follow with plugin_list to \
confirm what it added.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "GitHub repository 'owner/repo', URL, or local directory path" }
                },
                "required": ["source"]
            }),
        },
        ToolDef {
            name: "plugin_toggle".into(),
            description: "Turn an installed xConsole plugin on or off, immediately and \
without a restart. Disabling one removes its canvas nodes and its agent tools from the \
running session — including, potentially, tools you are part-way through using — so check \
plugin_list first and do not disable something because it looked unused.\n\
Use it to switch off a plugin that is misbehaving, or to enable one the user has just \
installed. It does not uninstall anything: the plugin stays on disk and keeps its \
settings.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "plugin_id": { "type": "string", "description": "Exact id from plugin_list." },
                    "enabled": { "type": "boolean", "description": "true enables it now; false disables it now." }
                },
                "required": ["plugin_id", "enabled"]
            }),
        },
    ]
}

fn terraform_params(apply: bool) -> Value {
    let extra_desc = if apply {
        "Extra terraform apply flags. Pass -auto-approve only when the user has explicitly \
agreed to this apply after seeing the plan; on its own it removes the last chance to stop \
a destroy."
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
        "plugin_list" => agent_plugin_list().await,
        "plugin_install" => agent_plugin_install(args).await,
        "plugin_toggle" => agent_plugin_toggle(args).await,
        "project_create" => project_create(ctx, args).await,
        "project_list" => project_list(ctx).await,
        "cloud_account_list" => cloud_account_list(ctx).await,
        "cloud_list_resources" => cloud_list_resources(ctx, args).await,
        "tfc_list_workspaces" => tfc_list_workspaces(ctx, args).await,
        "tfc_run_status" => tfc_run_status(ctx, args).await,
        "cloudflare_list_zones" => cloudflare_list_zones(ctx, args).await,
        "cloudflare_get_zone_analytics" => cloudflare_get_zone_analytics(ctx, args).await,
        "cloudflare_list_tunnels" => cloudflare_list_tunnels(ctx, args).await,
        "cloudflare_list_dns" => cloudflare_list_dns(ctx, args).await,
        "cloudflare_upsert_dns" => cloudflare_upsert_dns(ctx, args).await,
        "cloudflare_set_security_level" => cloudflare_set_security_level(ctx, args).await,
        "cloudflare_get_history" => cloudflare_get_history(ctx, args).await,
        "cloudflare_revert_action" => cloudflare_revert_action(ctx, args).await,
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

async fn resolve_cf_account(
    ctx: &ToolContext,
    args: &Value,
) -> Result<(crate::storage::models::CloudAccount, String, Option<String>), String> {
    let acc_id_opt = args
        .get("cloud_account_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let account = if let Some(id) = acc_id_opt {
        match ctx.db.get_cloud_account(id) {
            Ok(Some(a)) if a.kind == "cloudflare" => a,
            Ok(Some(_)) => return Err("error: account is not kind 'cloudflare'".into()),
            Ok(None) => return Err(format!("error: cloud account '{id}' not found")),
            Err(e) => return Err(format!("error: {e}")),
        }
    } else {
        let accounts = ctx.db.list_cloud_accounts().map_err(|e| e.to_string())?;
        accounts
            .into_iter()
            .find(|a| a.kind == "cloudflare")
            .ok_or_else(|| {
                "error: no connected Cloudflare account found in xConsole. Connect via Settings -> Cloud Accounts or Cloudflare Plugin.".to_string()
            })?
    };

    let token = crate::infra::cloudflare::load_cf_token(&account.id).map_err(|e| e.to_string())?;
    let cf_acc = account.project_id.clone();
    Ok((account, token, cf_acc))
}

async fn cloudflare_list_zones(ctx: &ToolContext, args: &Value) -> String {
    let (_account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    match crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
        Ok(zones) => {
            if zones.is_empty() {
                "No DNS zones found in Cloudflare account".into()
            } else {
                let lines: Vec<String> = zones
                    .iter()
                    .map(|z| {
                        let acc_name = z.account.as_ref().map(|a| a.name.as_str()).unwrap_or("-");
                        format!(
                            "- Domain: {} | Zone ID: {} | Status: {} | Account: {}",
                            z.name, z.id, z.status, acc_name
                        )
                    })
                    .collect();
                lines.join("\n")
            }
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn cloudflare_get_zone_analytics(ctx: &ToolContext, args: &Value) -> String {
    let (_account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };

    let mut zone_id = args
        .get("zone_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Auto-resolve domain name to Zone ID if user passed domain (e.g. "example.com")
    if zone_id.is_empty() || zone_id.contains('.') {
        let domain_query = zone_id.clone();
        if let Ok(zones) = crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
            if let Some(matched) = zones.iter().find(|z| !domain_query.is_empty() && (z.name == domain_query || z.id == domain_query)) {
                zone_id = matched.id.clone();
            } else if let Some(first) = zones.first() {
                zone_id = first.id.clone();
            }
        }
    }

    if zone_id.is_empty() {
        return "error: missing 'zone_id' and could not auto-detect active Cloudflare zone".into();
    }

    let since_minutes = args.get("since_minutes").and_then(|v| v.as_i64()).or_else(|| {
        args.get("since").and_then(|v| v.as_i64())
    });

    match crate::infra::cloudflare::get_zone_analytics(&token, &zone_id, since_minutes).await {
        Ok(val) => {
            let totals = val.get("totals").unwrap_or(&val);
            let req_all = totals.get("requests").and_then(|r| r.get("all")).and_then(|v| v.as_u64()).unwrap_or(0);
            let req_cached = totals.get("requests").and_then(|r| r.get("cached")).and_then(|v| v.as_u64()).unwrap_or(0);
            let req_uncached = totals.get("requests").and_then(|r| r.get("uncached")).and_then(|v| v.as_u64()).unwrap_or(0);
            let bw_all = totals.get("bandwidth").and_then(|r| r.get("all")).and_then(|v| v.as_u64()).unwrap_or(0);
            let bw_cached = totals.get("bandwidth").and_then(|r| r.get("cached")).and_then(|v| v.as_u64()).unwrap_or(0);
            let threats = totals.get("threats").and_then(|r| r.get("all")).and_then(|v| v.as_u64()).unwrap_or(0);
            let pageviews = totals.get("pageviews").and_then(|r| r.get("all")).and_then(|v| v.as_u64()).unwrap_or(0);
            let uniques = totals.get("uniques").and_then(|r| r.get("all")).and_then(|v| v.as_u64()).unwrap_or(0);
            let since = totals.get("since").and_then(|v| v.as_str()).unwrap_or("recent");
            let until = totals.get("until").and_then(|v| v.as_str()).unwrap_or("now");

            let cache_pct = if req_all > 0 { (req_cached as f64 / req_all as f64) * 100.0 } else { 0.0 };
            let bw_mb = (bw_all as f64) / (1024.0 * 1024.0);

            format!(
                "### Cloudflare Web Traffic & Analytics (Zone: {zone_id})\n\
                - Period: {since} -> {until}\n\
                - Total HTTP Requests: {req_all} ({cache_pct:.1}% cached: {req_cached} cached, {req_uncached} uncached)\n\
                - Total Bandwidth: {bw_mb:.2} MB ({bw_cached} bytes cached)\n\
                - Unique Visitors: {uniques}\n\
                - Total Pageviews: {pageviews}\n\
                - Blocked Security Threats: {threats}"
            )
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn cloudflare_list_tunnels(ctx: &ToolContext, args: &Value) -> String {
    let (account, token, cf_acc_opt) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let cf_acc = match cf_acc_opt.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            // Auto-resolve account ID from accounts list
            if let Ok(accs) = crate::infra::cloudflare::list_accounts(&token).await {
                if let Some(first) = accs.first() {
                    first.id.clone()
                } else {
                    return "error: account missing Cloudflare Account ID".into();
                }
            } else {
                return "error: account missing Cloudflare Account ID".into();
            }
        }
    };
    match crate::infra::cloudflare::list_tunnels(&token, &cf_acc).await {
        Ok(tunnels) => {
            if tunnels.is_empty() {
                format!("No tunnels found in Cloudflare account '{}'", account.name)
            } else {
                let lines: Vec<String> = tunnels
                    .iter()
                    .map(|t| {
                        format!(
                            "- {} (id: {}, status: {}, created: {})",
                            t.name,
                            t.id,
                            t.status.as_deref().unwrap_or("unknown"),
                            t.created_at.as_deref().unwrap_or("unknown")
                        )
                    })
                    .collect();
                lines.join("\n")
            }
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn cloudflare_list_dns(ctx: &ToolContext, args: &Value) -> String {
    let (_account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut zone_id = args.get("zone_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if zone_id.is_empty() || zone_id.contains('.') {
        let domain_query = zone_id.clone();
        if let Ok(zones) = crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
            if let Some(matched) = zones.iter().find(|z| !domain_query.is_empty() && (z.name == domain_query || z.id == domain_query)) {
                zone_id = matched.id.clone();
            } else if let Some(first) = zones.first() {
                zone_id = first.id.clone();
            }
        }
    }
    if zone_id.is_empty() {
        return "error: missing 'zone_id'".into();
    }
    match crate::infra::cloudflare::list_dns_records(&token, &zone_id).await {
        Ok(records) => {
            if records.is_empty() {
                "No DNS records found for zone".into()
            } else {
                let lines: Vec<String> = records
                    .iter()
                    .map(|r| {
                        format!(
                            "{} {} -> {} (proxied: {}, id: {})",
                            r.r#type, r.name, r.content, r.proxied, r.id
                        )
                    })
                    .collect();
                lines.join("\n")
            }
        }
        Err(e) => format!("error: {e}"),
    }
}

async fn cloudflare_upsert_dns(ctx: &ToolContext, args: &Value) -> String {
    let (account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut zone_id = match args.get("zone_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "error: missing 'zone_id'".into(),
    };
    if zone_id.contains('.') {
        if let Ok(zones) = crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
            if let Some(matched) = zones.iter().find(|z| z.name == zone_id || z.id == zone_id) {
                zone_id = matched.id.clone();
            }
        }
    }
    let r_type = match args.get("type").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'type'".into(),
    };
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'name'".into(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'content'".into(),
    };
    let proxied = args.get("proxied").and_then(|v| v.as_bool()).unwrap_or(false);
    let record_id = args.get("record_id").and_then(|v| v.as_str()).map(String::from);

    // Snapshot existing record for rollback
    let mut before_state: Option<String> = None;
    if let Some(rec_id) = &record_id {
        if !rec_id.is_empty() {
            if let Ok(records) = crate::infra::cloudflare::list_dns_records(&token, &zone_id).await {
                if let Some(existing) = records.into_iter().find(|r| r.id == *rec_id) {
                    let input_snap = crate::infra::cloudflare::CfDnsRecordInput {
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

    let input = crate::infra::cloudflare::CfDnsRecordInput {
        id: record_id,
        name: name.to_string(),
        r#type: r_type.to_string(),
        content: content.to_string(),
        proxied,
        ttl: 1,
        comment: Some("Managed by xConsole AI".into()),
    };

    match crate::infra::cloudflare::upsert_dns_record(&token, &zone_id, &input).await {
        Ok(rec) => {
            let is_update = before_state.is_some();
            let summary = if is_update {
                format!("Agent a actualizat DNS {} {} -> {}", rec.r#type, rec.name, rec.content)
            } else {
                format!("Agent a creat DNS nou {} {} ({})", rec.r#type, rec.name, rec.content)
            };

            let _ = ctx.db.create_cloudflare_audit_log(&crate::storage::models::CloudflareAuditLogInput {
                account_id: account.id.clone(),
                action_type: if is_update { "update_dns".to_string() } else { "create_dns".to_string() },
                target_id: Some(rec.id.clone()),
                target_name: Some(rec.name.clone()),
                summary,
                actor: "agent".to_string(),
                session_id: Some(ctx.session_id.clone()),
                before_state,
                after_state: serde_json::to_string(&rec).ok(),
            });

            format!("Saved DNS record {} {} -> {} (id: {})", rec.r#type, rec.name, rec.content, rec.id)
        }
        Err(e) => format!("error saving DNS record: {e}"),
    }
}

async fn cloudflare_set_security_level(ctx: &ToolContext, args: &Value) -> String {
    let (account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut zone_id = match args.get("zone_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "error: missing 'zone_id'".into(),
    };
    if zone_id.contains('.') {
        if let Ok(zones) = crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
            if let Some(matched) = zones.iter().find(|z| z.name == zone_id || z.id == zone_id) {
                zone_id = matched.id.clone();
            }
        }
    }
    let level = match args.get("level").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'level'".into(),
    };

    let old_settings = crate::infra::cloudflare::get_security_settings(&token, &zone_id).await.ok();
    let old_level = old_settings.map(|s| s.security_level);

    match crate::infra::cloudflare::set_security_level(&token, &zone_id, level).await {
        Ok(lvl) => {
            let _ = ctx.db.create_cloudflare_audit_log(&crate::storage::models::CloudflareAuditLogInput {
                account_id: account.id.clone(),
                action_type: "set_security_level".to_string(),
                target_id: Some(zone_id.to_string()),
                target_name: Some("WAF Security Level".to_string()),
                summary: format!("Agent a modificat nivelul de securitate la '{}'", lvl),
                actor: "agent".to_string(),
                session_id: Some(ctx.session_id.clone()),
                before_state: old_level,
                after_state: Some(lvl.clone()),
            });
            format!("Updated security level to '{lvl}' for zone {zone_id}")
        }
        Err(e) => format!("error setting security level: {e}"),
    }
}

async fn cloudflare_get_history(ctx: &ToolContext, args: &Value) -> String {
    let (account, _token, _cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    match ctx.db.list_cloudflare_audit_logs(&account.id, 20) {
        Ok(logs) => {
            if logs.is_empty() {
                format!("No Cloudflare edit history found for account '{}'.", account.name)
            } else {
                let lines: Vec<String> = logs
                    .iter()
                    .map(|l| {
                        let rev = if l.reverted { " [REVERTED]" } else { "" };
                        format!(
                            "- ID: {}\n  Action: {} (by {})\n  Summary: {}{}\n  Time: {}",
                            l.id, l.action_type, l.actor, l.summary, rev, l.created_at
                        )
                    })
                    .collect();
                lines.join("\n\n")
            }
        }
        Err(e) => format!("error retrieving history: {e}"),
    }
}

async fn cloudflare_revert_action(ctx: &ToolContext, args: &Value) -> String {
    let (account, token, cf_acc) = match resolve_cf_account(ctx, args).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let action_id = match args.get("action_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return "error: missing 'action_id'".into(),
    };

    let log = match ctx.db.get_cloudflare_audit_log(action_id) {
        Ok(Some(l)) => l,
        Ok(None) => return format!("error: history action ID '{action_id}' not found"),
        Err(e) => return format!("error: {e}"),
    };

    if log.reverted {
        return "error: this action was already reverted".into();
    }

    match log.action_type.as_str() {
        "create_dns" => {
            if let Some(target_id) = &log.target_id {
                let zone_id = account.region.clone().unwrap_or_default();
                if !zone_id.is_empty() {
                    let _ = crate::infra::cloudflare::delete_dns_record(&token, &zone_id, target_id).await;
                } else if let Ok(zones) = crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
                    for z in zones {
                        if crate::infra::cloudflare::delete_dns_record(&token, &z.id, target_id).await.is_ok() {
                            break;
                        }
                    }
                }
            }
        }
        "update_dns" | "delete_dns" => {
            if let Some(before_json) = &log.before_state {
                let snap: crate::infra::cloudflare::CfDnsRecordInput = match serde_json::from_str(before_json) {
                    Ok(s) => s,
                    Err(e) => return format!("error parsing previous DNS state: {e}"),
                };
                let zone_id = account.region.clone().unwrap_or_default();
                let actual_zone = if !zone_id.is_empty() {
                    zone_id
                } else {
                    let zones = match crate::infra::cloudflare::list_zones(&token, cf_acc.as_deref()).await {
                        Ok(z) => z,
                        Err(e) => return format!("error listing zones: {e}"),
                    };
                    zones.first().map(|z| z.id.clone()).unwrap_or_default()
                };
                if !actual_zone.is_empty() {
                    if let Err(e) = crate::infra::cloudflare::upsert_dns_record(&token, &actual_zone, &snap).await {
                        return format!("error restoring DNS record: {e}");
                    }
                }
            }
        }
        "set_security_level" => {
            if let Some(prev_level) = &log.before_state {
                if let Some(zone_id) = &log.target_id {
                    if let Err(e) = crate::infra::cloudflare::set_security_level(&token, zone_id, prev_level).await {
                        return format!("error restoring security level: {e}");
                    }
                }
            }
        }
        other => return format!("error: action type '{other}' does not support automatic revert"),
    }

    let _ = ctx.db.mark_cloudflare_audit_log_reverted(action_id);
    let _ = ctx.db.create_cloudflare_audit_log(&crate::storage::models::CloudflareAuditLogInput {
        account_id: account.id.clone(),
        action_type: "revert_action".to_string(),
        target_id: Some(action_id.to_string()),
        target_name: log.target_name.clone(),
        summary: format!("↩️ Agent a anulat: {}", log.summary),
        actor: "agent".to_string(),
        session_id: Some(ctx.session_id.clone()),
        before_state: None,
        after_state: None,
    });

    format!("Successfully reverted action '{}' ({})", log.summary, action_id)
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

async fn agent_plugin_list() -> String {
    let plugins = crate::plugins::list_plugins();
    if plugins.is_empty() {
        return "No plugins currently installed in xConsole.".into();
    }

    let mut out = format!("Installed xConsole Plugins ({}):\n", plugins.len());
    for p in plugins {
        let status = if p.enabled { "ENABLED" } else { "DISABLED" };
        let kind = if p.is_builtin { "builtin" } else { "community" };
        let tools_count = p.capabilities.agent_tools.as_ref().map(|t| t.len()).unwrap_or(0);
        out.push_str(&format!(
            "- {} (v{}) [{status} | {kind} | {tools_count} tools]: {}\n",
            p.name, p.version, p.description
        ));
    }
    out
}

async fn agent_plugin_install(args: &Value) -> String {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return "error: missing required argument 'source'".into(),
    };

    match crate::plugins::install_plugin(source) {
        Ok(p) => format!(
            "Successfully installed and activated plugin '{}' (v{}) by {}!\nCapabilities: {} agent tools available.",
            p.name,
            p.version,
            p.author,
            p.capabilities.agent_tools.as_ref().map(|t| t.len()).unwrap_or(0)
        ),
        Err(e) => format!("Error installing plugin from '{source}': {e}"),
    }
}

async fn agent_plugin_toggle(args: &Value) -> String {
    let plugin_id = match args.get("plugin_id").and_then(|v| v.as_str()) {
        Some(id) if !id.trim().is_empty() => id.trim(),
        _ => return "error: missing required argument 'plugin_id'".into(),
    };
    let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

    match crate::plugins::toggle_plugin(plugin_id, enabled) {
        Ok(_) => {
            let state = if enabled { "enabled" } else { "disabled" };
            format!("Plugin '{plugin_id}' is now {state}.")
        }
        Err(e) => format!("Error toggling plugin '{plugin_id}': {e}"),
    }
}


#[cfg(test)]
mod description_tests {
    use super::*;

    fn desc(name: &str) -> String {
        definitions()
            .into_iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
            .description
    }

    #[test]
    fn the_most_destructive_tool_here_says_to_plan_first() {
        // terraform_apply used to be 102 characters that never mentioned a plan. It is
        // the one tool in this file that can delete a database, and "requires approval"
        // is not the same as telling the agent what to look at before asking for one.
        let d = desc("terraform_apply");
        assert!(d.contains("terraform_plan"), "{d}");
        assert!(d.to_lowercase().contains("destroy"), "{d}");
        assert!(d.contains("tfc_run_status"), "a queued TFC apply is not a finished one");
    }

    #[test]
    fn changing_public_dns_says_what_goes_wrong_and_how_to_undo_it() {
        let d = desc("cloudflare_upsert_dns");
        assert!(d.contains("cloudflare_list_dns"), "read what is there first: {d}");
        assert!(d.contains("record_id"), "the create-vs-update trap: {d}");
        assert!(d.contains("cloudflare_revert_action"), "{d}");
    }

    #[test]
    fn a_whole_file_overwrite_says_it_is_a_whole_file_overwrite() {
        // project_write was 39 characters, and a model that reads "write a file in a
        // project" has no reason to read the file first.
        let d = desc("project_write");
        assert!(d.contains("project_read"), "{d}");
        assert!(d.contains("terraform_plan"), "writing HCL changes nothing on its own: {d}");
    }

    #[test]
    fn an_asynchronous_run_status_tool_says_not_to_poll_it() {
        let d = desc("tfc_run_status");
        assert!(d.to_lowercase().contains("do not poll"), "{d}");
    }

    #[test]
    fn every_tool_in_this_file_says_more_than_its_own_name() {
        // The floor: a description that is only a restatement of the tool name tells a
        // model nothing it did not already have from the name.
        for def in definitions() {
            assert!(
                def.description.len() >= 90,
                "{} is described in {} characters",
                def.name,
                def.description.len()
            );
        }
    }
}

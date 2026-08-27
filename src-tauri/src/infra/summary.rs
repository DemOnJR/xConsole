//! Compact infra inventory for the agent system prompt.

use crate::storage::Db;

/// Short summary of cloud accounts, active plugins, and Terraform projects for agent context.
pub fn format_infra_summary(db: &Db) -> String {
    let mut lines = Vec::new();

    // 1. Active Plugins
    let plugins = crate::plugins::list_plugins();
    let active_plugins: Vec<_> = plugins.into_iter().filter(|p| p.enabled).collect();
    if !active_plugins.is_empty() {
        lines.push(format!("Active Plugins ({}):", active_plugins.len()));
        for p in &active_plugins {
            let tools_count = p.capabilities.agent_tools.as_ref().map(|t| t.len()).unwrap_or(0);
            lines.push(format!("  - {} (id={}, tools={})", p.name, p.id, tools_count));
        }
    }

    // 2. Cloud Accounts
    if let Ok(accounts) = db.list_cloud_accounts() {
        if accounts.is_empty() {
            lines.push("Cloud accounts: none".into());
        } else {
            lines.push(format!("Cloud accounts ({}):", accounts.len()));
            for a in &accounts {
                let creds = if a.has_secret { "ok" } else { "missing creds" };
                let extra = match a.kind.as_str() {
                    "aws" => a.region.as_deref().unwrap_or("us-east-1"),
                    "gcp" => a.project_id.as_deref().unwrap_or("-"),
                    "tfc" => a.organization.as_deref().unwrap_or("-"),
                    "cloudflare" => a.project_id.as_deref().unwrap_or("connected"),
                    _ => "-",
                };
                lines.push(format!(
                    "  - {} id={} kind={} {} ({})",
                    a.name, a.id, a.kind, creds, extra
                ));
            }
        }
    }

    // 3. Terraform Projects
    if let Ok(projects) = db.list_infra_projects() {
        if projects.is_empty() {
            lines.push("Terraform projects: none".into());
        } else {
            lines.push(format!("Terraform projects ({}):", projects.len()));
            for p in &projects {
                let runner = p
                    .default_vps_id
                    .as_deref()
                    .unwrap_or("local/TFC");
                lines.push(format!(
                    "  - {} slug={} template={} backend={} runner={}",
                    p.name, p.slug, p.template, p.backend, runner
                ));
            }
        }
    }

    lines.push(
        "Integrations Guidance:\n\
         - Cloudflare: Use cloudflare_list_zones to list domains, cloudflare_get_zone_analytics for live traffic/bandwidth/requests, cloudflare_list_tunnels, cloudflare_list_dns, cloudflare_upsert_dns, and cloudflare_set_security_level.\n\
         - Database: Use db_* tools to inspect tables, schemas, and query databases.\n\
         - Telemetry: Use get_system_telemetry to inspect hardware and AI cache metrics.\n\
         - Infra execution: no VPS targets → local terraform or TFC remote runs; select VPS targets for SSH/run_command."
            .into(),
    );

    lines.join("\n")
}

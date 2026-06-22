//! Compact infra inventory for the agent system prompt.

use crate::storage::Db;

/// Short summary of cloud accounts and Terraform projects for agent context.
pub fn format_infra_summary(db: &Db) -> String {
    let mut lines = Vec::new();

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
                    _ => "-",
                };
                lines.push(format!(
                    "  - {} id={} kind={} {} ({})",
                    a.name, a.id, a.kind, creds, extra
                ));
            }
        }
    }

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
        "Infra execution: no VPS targets → local terraform or TFC remote runs; \
select VPS targets for SSH/run_command and VPS terraform runner."
            .into(),
    );

    lines.join("\n")
}

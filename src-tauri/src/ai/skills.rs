//! Category-organized skills (SKILL.md playbooks), mirroring Hermes' skills
//! layout. Phase 7 adds the full index, view, and management tools; this module
//! provides the compact in-prompt index used by the context builder.

use std::path::Path;

use crate::ai::AgentHome;

/// A discovered skill.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Skill {
    pub category: String,
    pub name: String,
    /// First line / short description parsed from the SKILL.md front matter.
    pub description: String,
}

/// Walk `skills/<category>/<name>/SKILL.md` and return all skills.
pub fn discover(home: &AgentHome) -> Vec<Skill> {
    let root = home.skills_dir();
    let mut out = Vec::new();
    let Ok(cats) = std::fs::read_dir(&root) else {
        return out;
    };
    for cat in cats.flatten() {
        if !cat.path().is_dir() {
            continue;
        }
        let category = cat.file_name().to_string_lossy().to_string();
        if let Ok(skills) = std::fs::read_dir(cat.path()) {
            for s in skills.flatten() {
                let skill_md = s.path().join("SKILL.md");
                if skill_md.exists() {
                    out.push(Skill {
                        category: category.clone(),
                        name: s.file_name().to_string_lossy().to_string(),
                        description: first_description(&skill_md),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| (a.category.as_str(), a.name.as_str()).cmp(&(&b.category, &b.name)));
    out
}

/// Compact, grouped-by-category skill index for the system prompt. Empty when
/// no skills are installed (no token cost).
pub fn system_index(home: &AgentHome) -> String {
    let skills = discover(home);
    if skills.is_empty() {
        return String::new();
    }
    let mut by_cat: std::collections::BTreeMap<String, Vec<Skill>> = std::collections::BTreeMap::new();
    for s in skills {
        by_cat.entry(s.category.clone()).or_default().push(s);
    }
    let mut out = String::from(
        "# Available skills\nLoad a skill's SKILL.md before using it. Categories:\n",
    );
    for (cat, items) in by_cat {
        out.push_str(&format!("- {}: ", cat));
        let names: Vec<String> = items
            .iter()
            .map(|s| {
                if s.description.is_empty() {
                    s.name.clone()
                } else {
                    format!("{} ({})", s.name, s.description)
                }
            })
            .collect();
        out.push_str(&names.join(", "));
        out.push('\n');
    }
    out
}

/// Names-only skill index for ponytail / auto-compact mode (minimal tokens).
pub fn system_index_minimal(home: &AgentHome) -> String {
    let skills = discover(home);
    if skills.is_empty() {
        return String::new();
    }
    let mut by_cat: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for s in skills {
        by_cat.entry(s.category).or_default().push(s.name);
    }
    let mut out = String::from("# Skills (names only — load SKILL.md before use)\n");
    for (cat, names) in by_cat {
        out.push_str(&format!("- {}: {}\n", cat, names.join(", ")));
    }
    out
}

/// Read a skill's full SKILL.md body.
pub fn read_skill(home: &AgentHome, category: &str, name: &str) -> Option<String> {
    let path = home.skills_dir().join(category).join(name).join("SKILL.md");
    std::fs::read_to_string(path).ok()
}

fn slug(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Create or overwrite a skill's SKILL.md. Category and name are slugified.
pub fn save_skill(
    home: &AgentHome,
    category: &str,
    name: &str,
    content: &str,
) -> Result<(), String> {
    let cat = slug(category);
    let nm = slug(name);
    if cat.is_empty() || nm.is_empty() {
        return Err("category and name are required".into());
    }
    let dir = home.skills_dir().join(&cat).join(&nm);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("SKILL.md"), content).map_err(|e| e.to_string())
}

/// Delete a skill directory.
pub fn delete_skill(home: &AgentHome, category: &str, name: &str) -> Result<(), String> {
    let dir = home.skills_dir().join(slug(category)).join(slug(name));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Bundled skills shipped with xConsole (ponytail, terraform, …). Installed once if missing.
pub fn ensure_bundled(home: &AgentHome) {
    const SKILLS: &[(&str, &str, &str)] = &[
        (
            "meta",
            "ponytail",
            include_str!("../../bundled-skills/meta/ponytail/SKILL.md"),
        ),
        (
            "infra",
            "terraform-vps",
            include_str!("../../bundled-skills/infra/terraform-vps/SKILL.md"),
        ),
        (
            "infra",
            "terraform-aws",
            include_str!("../../bundled-skills/infra/terraform-aws/SKILL.md"),
        ),
        (
            "infra",
            "terraform-gcp",
            include_str!("../../bundled-skills/infra/terraform-gcp/SKILL.md"),
        ),
        (
            "infra",
            "terraform-tfc",
            include_str!("../../bundled-skills/infra/terraform-tfc/SKILL.md"),
        ),
    ];
    for (cat, name, body) in SKILLS {
        let _ = save_skill(home, cat, name, body);
    }
}

/// Seed starter DevOps skills when the devops category is empty.
pub fn seed_defaults(home: &AgentHome) {
    ensure_bundled(home);
    if discover(home).iter().any(|s| s.category == "devops") {
        return;
    }
    let defaults: &[(&str, &str, &str)] = &[
        (
            "devops",
            "service-health-check",
            "---\ndescription: Inspect a systemd service's status, recent logs, and restarts.\n---\n\n# Service health check\n\nGiven a service name:\n1. Run `systemctl status <svc>` to see state and recent log lines.\n2. Run `journalctl -u <svc> -n 100 --no-pager` for recent logs.\n3. Check resource use with `systemctl show <svc> -p MemoryCurrent,CPUUsageNSec`.\n4. Summarize: is it active, any recent failures, and the likely cause.\nDo not restart anything without confirming with the user.\n",
        ),
        (
            "devops",
            "disk-cleanup",
            "---\ndescription: Find and safely reclaim disk space on a server.\n---\n\n# Disk cleanup\n\n1. `df -h` to see which filesystem is full.\n2. `du -xh <mount> | sort -rh | head -30` to find large directories.\n3. Identify safe-to-remove items (old logs, apt/yum caches, orphaned docker images).\n4. Propose exact removal commands and wait for approval before deleting.\n",
        ),
        (
            "devops",
            "nginx-deploy-check",
            "---\ndescription: Validate an nginx config and reload safely.\n---\n\n# Nginx deploy check\n\n1. `nginx -t` to validate the configuration.\n2. Only if valid, `systemctl reload nginx`.\n3. Verify with `systemctl status nginx` and a `curl -I` against the site.\nNever reload if `nginx -t` fails.\n",
        ),
    ];
    for (cat, name, body) in defaults {
        let _ = save_skill(home, cat, name, body);
    }
}

fn first_description(skill_md: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(skill_md) else {
        return String::new();
    };
    // Prefer a `description:` front-matter line; else the first non-heading line.
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("description:") {
            return rest.trim().trim_matches('"').to_string();
        }
    }
    for line in text.lines() {
        let l = line.trim();
        if !l.is_empty() && !l.starts_with('#') && l != "---" {
            return l.chars().take(80).collect();
        }
    }
    String::new()
}

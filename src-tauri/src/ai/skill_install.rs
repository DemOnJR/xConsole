//! Fetching skills from the wider ecosystem so the skill set can grow over time.
//!
//! This module only *fetches* and helps name a skill; the security scan
//! ([`crate::ai::skill_scan`]) and the install gate (trusted → auto, untrusted →
//! approval, failing scan → blocked) are orchestrated by the `skill_install` tool
//! in [`crate::ai::tools`], which has the approval context.

use serde_json::Value;

use crate::ai::web_tools;

/// Rewrite a github.com blob/tree URL to its raw.githubusercontent.com form;
/// other URLs pass through unchanged.
pub fn to_raw_url(source: &str) -> String {
    let s = source.trim();
    if let Some(rest) = s.strip_prefix("https://github.com/") {
        let rewritten = rest.replacen("/blob/", "/", 1).replacen("/tree/", "/", 1);
        return format!("https://raw.githubusercontent.com/{rewritten}");
    }
    s.to_string()
}

/// Fetch a skill's `SKILL.md` text from a URL (raw, github blob/tree, or a
/// directory URL — `/SKILL.md` is appended when the URL isn't already a `.md`).
pub async fn fetch_skill_md(source: &str) -> Result<String, String> {
    let mut url = to_raw_url(source);
    if !url.to_lowercase().ends_with(".md") {
        url = format!("{}/SKILL.md", url.trim_end_matches('/'));
    }
    let validated = web_tools::validate_public_url(&url)?;
    let client = web_tools::http_client()?;
    let resp = client
        .get(validated)
        .send()
        .await
        .map_err(|e| format!("error: fetch failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("error: HTTP {} fetching {url}", resp.status()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("error: reading skill body: {e}"))?;
    if text.trim().is_empty() {
        return Err("error: fetched skill is empty".into());
    }
    if text.len() > 256 * 1024 {
        return Err("error: skill file too large".into());
    }
    Ok(text)
}

/// Derive a sensible `name` for a fetched skill from its source URL (the path
/// segment that holds the SKILL.md, e.g. `.../pdf/SKILL.md` → `pdf`).
pub fn derive_name(source: &str) -> String {
    let trimmed = source.trim().trim_end_matches('/');
    let without_file = trimmed
        .strip_suffix("/SKILL.md")
        .or_else(|| trimmed.strip_suffix("/skill.md"))
        .unwrap_or(trimmed);
    without_file
        .rsplit('/')
        .find(|seg| !seg.is_empty() && *seg != "main" && *seg != "master")
        .unwrap_or("skill")
        .to_string()
}

/// List the top-level skill directories in the official anthropics/skills repo
/// (best-effort, read-only) so the agent can discover what's available.
pub async fn list_official_skills() -> String {
    let client = match web_tools::http_client() {
        Ok(c) => c,
        Err(e) => return e,
    };
    let url = "https://api.github.com/repos/anthropics/skills/contents/";
    let resp = match client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("error: listing failed: {e}"),
    };
    if !resp.status().is_success() {
        return format!("error: GitHub API HTTP {}", resp.status());
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return format!("error: invalid listing response: {e}"),
    };
    let Some(arr) = body.as_array() else {
        return "error: unexpected listing format".into();
    };
    let mut dirs: Vec<String> = arr
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("dir"))
        .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        return "No skills found in anthropics/skills.".into();
    }
    format!(
        "Official skills (github.com/anthropics/skills). Install one with skill_install using its \
         folder URL, e.g. https://github.com/anthropics/skills/tree/main/<name>:\n{}",
        dirs.iter().map(|d| format!("- {d}")).collect::<Vec<_>>().join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_github_blob_to_raw() {
        assert_eq!(
            to_raw_url("https://github.com/anthropics/skills/tree/main/pdf"),
            "https://raw.githubusercontent.com/anthropics/skills/main/pdf"
        );
        assert_eq!(
            to_raw_url("https://github.com/anthropics/skills/blob/main/pdf/SKILL.md"),
            "https://raw.githubusercontent.com/anthropics/skills/main/pdf/SKILL.md"
        );
        assert_eq!(to_raw_url("https://example.com/x"), "https://example.com/x");
    }

    #[test]
    fn derives_name_from_url() {
        assert_eq!(
            derive_name("https://github.com/anthropics/skills/tree/main/pdf"),
            "pdf"
        );
        assert_eq!(
            derive_name("https://raw.githubusercontent.com/anthropics/skills/main/docx/SKILL.md"),
            "docx"
        );
    }
}

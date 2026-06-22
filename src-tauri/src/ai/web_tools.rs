//! Read-only HTTP tools for real-time public internet access (weather, docs, etc.).

use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use crate::ai::provider::ToolDef;

const MAX_BODY: usize = 48_000;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "web_search".into(),
            description: "Search the public web for current information (weather, news, facts). \
Returns a short summary from DuckDuckGo. Prefer this before guessing.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query, e.g. 'weather in Berlin today'."
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "web_fetch".into(),
            description: "Fetch a public HTTP(S) URL and return plain text (HTML stripped). \
For weather use https://wttr.in/City?format=3 (replace City, URL-encode spaces).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full http:// or https:// URL."
                    }
                },
                "required": ["url"]
            }),
        },
    ]
}

pub async fn dispatch(name: &str, args: &Value) -> String {
    match name {
        "web_search" => web_search(args).await,
        "web_fetch" => web_fetch(args).await,
        other => format!("error: unknown web tool '{other}'"),
    }
}

pub fn is_web_tool(name: &str) -> bool {
    matches!(name, "web_search" | "web_fetch")
}

async fn web_search(args: &Value) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q.trim(),
        _ => return "error: missing 'query'".into(),
    };

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return e,
    };

    let resp = match client
        .get("https://api.duckduckgo.com/")
        .query(&[("q", query), ("format", "json"), ("no_redirect", "1"), ("no_html", "1")])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("error: search request failed: {e}"),
    };

    if !resp.status().is_success() {
        return format!("error: search HTTP {}", resp.status());
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return format!("error: invalid search response: {e}"),
    };

    let mut parts = Vec::new();
    if let Some(abs) = body.get("AbstractText").and_then(|v| v.as_str()) {
        if !abs.is_empty() {
            parts.push(abs.to_string());
        }
    }
    if let Some(ans) = body.get("Answer").and_then(|v| v.as_str()) {
        if !ans.is_empty() {
            parts.push(format!("Answer: {ans}"));
        }
    }
    if let Some(topic) = body.get("Heading").and_then(|v| v.as_str()) {
        if !topic.is_empty() && parts.is_empty() {
            parts.push(topic.to_string());
        }
    }
    if let Some(related) = body.get("RelatedTopics").and_then(|v| v.as_array()) {
        for item in related.iter().take(5) {
            if let Some(text) = item.get("Text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
    }

    if parts.is_empty() {
        return format!(
            "No instant answer for \"{query}\". Try web_fetch with a specific URL \
(e.g. https://wttr.in/?format=3 for local weather)."
        );
    }

    truncate_text(&parts.join("\n"), MAX_BODY)
}

async fn web_fetch(args: &Value) -> String {
    let url_str = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) if !u.trim().is_empty() => u.trim(),
        _ => return "error: missing 'url'".into(),
    };

    let url = match validate_public_url(url_str) {
        Ok(u) => u,
        Err(e) => return e,
    };

    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return e,
    };

    let resp = match client.get(url.clone()).send().await {
        Ok(r) => r,
        Err(e) => return format!("error: fetch failed: {e}"),
    };

    if !resp.status().is_success() {
        return format!("error: HTTP {} for {url}", resp.status());
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return format!("error: read body: {e}"),
    };

    if bytes.len() > MAX_BODY {
        return format!(
            "error: response too large ({} bytes, max {MAX_BODY})",
            bytes.len()
        );
    }

    let raw = String::from_utf8_lossy(&bytes);
    let text = if content_type.contains("html") || raw.trim_start().starts_with('<') {
        html_to_text(&raw)
    } else {
        raw.into_owned()
    };

    truncate_text(&text, MAX_BODY)
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent("xConsole-agent/1.0 (+https://github.com/xconsole)")
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| format!("error: http client: {e}"))
}

fn validate_public_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("error: invalid url: {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("error: only http/https URLs are allowed".into());
    }
    if url.username() != "" || url.password().is_some() {
        return Err("error: URL credentials are not allowed".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "error: URL must have a host".to_string())?
        .to_lowercase();
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err("error: local/private hosts are not allowed".into());
    }
    if host == "metadata.google.internal" || host == "169.254.169.254" {
        return Err("error: metadata endpoints are not allowed".into());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Err("error: private IP addresses are not allowed".into());
        }
    }
    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4 == Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified() || is_unique_local(v6),
    }
}

fn is_unique_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(8192));
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut skip_until = None::<&str>;

    for ch in html.chars() {
        match ch {
            '<' if !in_tag => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag = tag_buf.trim().to_lowercase();
                if tag.starts_with("script") || tag.starts_with("style") {
                    skip_until = if tag.starts_with("/") {
                        None
                    } else {
                        Some(if tag.starts_with("script") {
                            "script"
                        } else {
                            "style"
                        })
                    };
                } else if tag.starts_with("/script") || tag.starts_with("/style") {
                    skip_until = None;
                } else if tag.starts_with("br") || tag.starts_with("p") || tag.starts_with("div") {
                    out.push('\n');
                }
                tag_buf.clear();
            }
            _ if in_tag => tag_buf.push(ch),
            _ => {
                if skip_until.is_none() {
                    out.push(ch);
                }
            }
        }
    }

    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("> <", "> <")
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    format!("{}… [truncated]", &text[..max])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost() {
        assert!(validate_public_url("http://localhost/test").is_err());
        assert!(validate_public_url("http://127.0.0.1/").is_err());
    }

    #[test]
    fn allows_public_https() {
        assert!(validate_public_url("https://wttr.in/Berlin?format=3").is_ok());
    }
}

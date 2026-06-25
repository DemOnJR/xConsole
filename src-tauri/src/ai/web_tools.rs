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
For weather use https://wttr.in/City?format=3 (replace City, URL-encode spaces). \
For weather at the user's own location, https://wttr.in/?format=3 auto-detects by IP — \
or call geo_locate first to get the city.".into(),
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
        ToolDef {
            name: "geo_locate".into(),
            description: "Resolve the user's approximate location (city, region, country, latitude, \
longitude, timezone) from their public IP address. Use this for 'my location', 'near me', \
'my position', 'my timezone', or local weather when the user did not name a city. \
Accuracy is city-level only.".into(),
            parameters: json!({"type": "object", "properties": {}}),
        },
    ]
}

pub async fn dispatch(name: &str, args: &Value) -> String {
    match name {
        "web_search" => web_search(args).await,
        "web_fetch" => web_fetch(args).await,
        "geo_locate" => geo_locate().await,
        other => format!("error: unknown web tool '{other}'"),
    }
}

pub fn is_web_tool(name: &str) -> bool {
    matches!(name, "web_search" | "web_fetch" | "geo_locate")
}

/// A normalized location parsed from one of several geo-IP providers.
struct GeoLocation {
    ip: String,
    city: String,
    region: String,
    country: String,
    lat: String,
    lon: String,
    timezone: String,
}

impl GeoLocation {
    fn is_usable(&self) -> bool {
        !self.city.is_empty() || !self.region.is_empty() || !self.country.is_empty()
    }

    fn render(&self) -> String {
        let mut lines = vec![format!(
            "Approximate location (city-level, from IP {}):",
            self.ip
        )];
        lines.push(format!("City: {}", self.city));
        lines.push(format!("Region: {}", self.region));
        lines.push(format!("Country: {}", self.country));
        if !self.lat.is_empty() && !self.lon.is_empty() {
            lines.push(format!("Coordinates: {}, {}", self.lat, self.lon));
        }
        if !self.timezone.is_empty() {
            lines.push(format!("Timezone: {}", self.timezone));
        }
        lines.join("\n")
    }
}

fn jstr(body: &Value, key: &str) -> String {
    body.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn jnum(body: &Value, key: &str) -> String {
    match body.get(key) {
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

/// Resolve the user's approximate location from their public IP (city-level).
/// Used for "my position", "near me", local weather without a named city.
/// Tries multiple key-free providers so one rate-limit doesn't break the tool.
async fn geo_locate() -> String {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut last_err = String::from("error: no geolocation provider returned a location");

    // (url, parser) pairs, tried in order until one yields a usable location.
    let providers: &[(&str, fn(&Value) -> GeoLocation)] = &[
        ("https://ipapi.co/json/", parse_ipapi_co),
        ("https://ipwho.is/", parse_ipwho_is),
    ];

    for (url, parse) in providers {
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(body) => {
                    // Provider-level error flags (rate limit, etc.) — try the next one.
                    let rate_limited = body.get("error").and_then(|v| v.as_bool()) == Some(true)
                        || body.get("success").and_then(|v| v.as_bool()) == Some(false);
                    if rate_limited {
                        let reason = body
                            .get("reason")
                            .or_else(|| body.get("message"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("rate limited");
                        last_err = format!("error: geolocation failed: {reason}");
                        continue;
                    }
                    let loc = parse(&body);
                    if loc.is_usable() {
                        return loc.render();
                    }
                    last_err =
                        "error: geolocation returned no location (IP may be private or blocked)"
                            .into();
                }
                Err(e) => last_err = format!("error: invalid geolocation response: {e}"),
            },
            Ok(resp) => last_err = format!("error: geolocation HTTP {}", resp.status()),
            Err(e) => last_err = format!("error: geolocation request failed: {e}"),
        }
    }

    last_err
}

fn parse_ipapi_co(body: &Value) -> GeoLocation {
    GeoLocation {
        ip: jstr(body, "ip"),
        city: jstr(body, "city"),
        region: jstr(body, "region"),
        country: jstr(body, "country_name"),
        lat: jnum(body, "latitude"),
        lon: jnum(body, "longitude"),
        timezone: jstr(body, "timezone"),
    }
}

fn parse_ipwho_is(body: &Value) -> GeoLocation {
    // ipwho.is nests the IANA timezone under "timezone": { "id": "Europe/Rome" }.
    let timezone = body
        .get("timezone")
        .and_then(|tz| tz.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    GeoLocation {
        ip: jstr(body, "ip"),
        city: jstr(body, "city"),
        region: jstr(body, "region"),
        country: jstr(body, "country"),
        lat: jnum(body, "latitude"),
        lon: jnum(body, "longitude"),
        timezone,
    }
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

    // Primary: real web results (titles + snippets). DuckDuckGo's instant-answer API
    // returns nothing for most queries (place names, "X weather", etc.), so scrape the
    // HTML results endpoint, which returns actual search hits.
    if let Ok(results) = ddg_html_results(&client, query).await {
        if !results.is_empty() {
            let mut block = format!("Top web results for \"{query}\":");
            for (i, r) in results.iter().take(6).enumerate() {
                block.push_str(&format!("\n{}. {r}", i + 1));
            }
            return truncate_text(&block, MAX_BODY);
        }
    }

    // Fallback: instant answer (definitions, calculations, direct facts).
    if let Some(ia) = ddg_instant_answer(&client, query).await {
        return truncate_text(&ia, MAX_BODY);
    }

    format!(
        "No results for \"{query}\". For weather, web_fetch https://wttr.in/CITY?format=3 \
(URL-encode spaces as +). For a specific site, call web_fetch with its URL directly."
    )
}

/// Real search results (title — snippet) scraped from DuckDuckGo's HTML endpoint.
async fn ddg_html_results(client: &reqwest::Client, query: &str) -> Result<Vec<String>, String> {
    let resp = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        // DuckDuckGo serves the HTML results only to browser-like user agents.
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| format!("error: search request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("error: search HTTP {}", resp.status()));
    }
    let html = resp
        .text()
        .await
        .map_err(|e| format!("error: read search body: {e}"))?;

    let titles = anchor_inner_texts(&html, "result__a");
    let snippets = anchor_inner_texts(&html, "result__snippet");
    let mut out = Vec::new();
    for i in 0..titles.len() {
        let title = titles.get(i).cloned().unwrap_or_default();
        let snip = snippets.get(i).cloned().unwrap_or_default();
        let line = match (title.is_empty(), snip.is_empty()) {
            (false, false) => format!("{title} — {snip}"),
            (false, true) => title,
            (true, false) => snip,
            (true, true) => continue,
        };
        out.push(line);
        if out.len() >= 6 {
            break;
        }
    }
    Ok(out)
}

/// DuckDuckGo Instant Answer (definitions, calculations, direct facts). Often empty.
async fn ddg_instant_answer(client: &reqwest::Client, query: &str) -> Option<String> {
    let body: Value = client
        .get("https://api.duckduckgo.com/")
        .query(&[("q", query), ("format", "json"), ("no_redirect", "1"), ("no_html", "1")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let mut parts = Vec::new();
    for key in ["AbstractText", "Answer", "Definition"] {
        if let Some(s) = body.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Inner text of every `<a class="<class>" …>…</a>` anchor (tags stripped, entities decoded).
fn anchor_inner_texts(html: &str, class: &str) -> Vec<String> {
    let needle = format!("class=\"{class}\"");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find(&needle) {
        let cls = from + rel;
        let Some(gt) = html[cls..].find('>') else { break };
        let inner_start = cls + gt + 1;
        let Some(close) = html[inner_start..].find("</a>") else {
            from = inner_start;
            continue;
        };
        let inner = &html[inner_start..inner_start + close];
        let text = decode_entities(html_to_text(inner).trim());
        if !text.trim().is_empty() {
            out.push(text.trim().to_string());
        }
        from = inner_start + close + 4;
    }
    out
}

/// Decode the handful of HTML entities that show up in search snippets.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&nbsp;", " ")
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

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent("xConsole-agent/1.0 (+https://github.com/xconsole)")
        // Re-validate EVERY redirect hop. reqwest follows 3xx responses without re-checking,
        // so without this a public URL could 30x-redirect to 127.0.0.1 / a metadata IP and
        // slip past validate_public_url (which only ever sees the first URL). We keep the same
        // 3-hop budget the old Policy::limited(3) enforced.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects".to_string());
            }
            if let Some(reason) = blocked_target(attempt.url()) {
                return attempt.error(format!("blocked redirect target: {reason}"));
            }
            attempt.follow()
        }))
        .build()
        .map_err(|e| format!("error: http client: {e}"))
}

/// Why a URL points at a local/private/metadata target, or `None` if it looks public.
/// Used for BOTH the initial URL (via [`validate_public_url`]) and every redirect hop, so the
/// SSRF guard can't be sidestepped with a 30x redirect.
fn blocked_target(url: &reqwest::Url) -> Option<&'static str> {
    let host = match url.host_str() {
        Some(h) => h.to_lowercase(),
        None => return Some("URL must have a host"),
    };
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Some("local/private hosts are not allowed");
    }
    if host == "metadata.google.internal" || host == "169.254.169.254" {
        return Some("metadata endpoints are not allowed");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Some("private IP addresses are not allowed");
        }
    }
    None
}

pub fn validate_public_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("error: invalid url: {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("error: only http/https URLs are allowed".into());
    }
    if url.username() != "" || url.password().is_some() {
        return Err("error: URL credentials are not allowed".into());
    }
    if let Some(reason) = blocked_target(&url) {
        return Err(format!("error: {reason}"));
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
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4 == Ipv4Addr::new(169, 254, 169, 254)
                || matches!(v4.octets(), [100, 64..=127, _, _]) // 100.64.0.0/10 CGNAT / Tailscale
                || matches!(v4.octets(), [192, 0, 0, _]) // 192.0.0.0/24 IETF protocol assignments
                || matches!(v4.octets(), [198, 18..=19, _, _]) // 198.18.0.0/15 benchmarking
        }
        IpAddr::V6(v6) => {
            // An IPv4-mapped address like ::ffff:169.254.169.254 must be judged
            // by its embedded v4 address, or the guard is trivially bypassed.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(IpAddr::V4(v4));
            }
            v6.is_loopback() || v6.is_unspecified() || is_unique_local(v6) || is_v6_link_local(v6)
        }
    }
}

fn is_unique_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

/// IPv6 link-local: fe80::/10.
fn is_v6_link_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
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

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    format!("{}… [truncated]", super::text::truncate_bytes(text, max))
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
    fn parses_ddg_html_anchors() {
        let html = r#"<a rel="nofollow" class="result__a" href="//x">Marina di Tor San Lorenzo &amp; beach</a>
            <a class="result__snippet" href="//y">A <b>coastal</b> town in Lazio near Rome.</a>"#;
        assert_eq!(
            anchor_inner_texts(html, "result__a"),
            vec!["Marina di Tor San Lorenzo & beach"]
        );
        assert_eq!(
            anchor_inner_texts(html, "result__snippet"),
            vec!["A coastal town in Lazio near Rome."]
        );
    }

    #[test]
    fn allows_public_https() {
        assert!(validate_public_url("https://wttr.in/Berlin?format=3").is_ok());
    }

    #[test]
    fn blocks_ipv4_mapped_ipv6() {
        // IPv4-mapped IPv6 must not bypass the metadata/loopback guard.
        assert!(validate_public_url("http://[::ffff:169.254.169.254]/latest/meta-data").is_err());
        assert!(validate_public_url("http://[::ffff:127.0.0.1]/").is_err());
    }

    #[test]
    fn blocks_extra_private_ranges() {
        assert!(validate_public_url("http://100.64.0.1/").is_err()); // CGNAT / Tailscale
        assert!(validate_public_url("http://198.18.0.1/").is_err()); // benchmarking
        assert!(validate_public_url("http://192.0.0.1/").is_err()); // IETF protocol
        assert!(validate_public_url("http://[fe80::1]/").is_err()); // v6 link-local
    }

    #[test]
    fn redirect_predicate_blocks_private_targets() {
        // blocked_target backs the per-hop redirect guard; private/metadata => blocked, public => ok.
        let blocked = |u: &str| blocked_target(&reqwest::Url::parse(u).unwrap()).is_some();
        assert!(blocked("http://127.0.0.1:11434/"));
        assert!(blocked("http://169.254.169.254/latest/meta-data"));
        assert!(blocked("http://10.0.0.5/"));
        assert!(!blocked("https://example.com/"));
    }

    #[test]
    fn truncate_text_handles_multibyte() {
        let s = "ä".repeat(100); // 200 bytes
        // Must not panic slicing mid-codepoint, and must stay within budget.
        let out = truncate_text(&s, 51);
        assert!(out.starts_with('ä'));
    }
}

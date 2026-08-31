//! Read-only HTTP tools for real-time public internet access (weather, docs, etc.).

use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use crate::ai::provider::ToolDef;

const MAX_BODY: usize = 48_000;
/// How much can be downloaded before the page is refused unread.
///
/// Deliberately far above MAX_BODY: this guards memory, not the reply. What the
/// model sees is capped separately, after the markup has been stripped.
const MAX_DOWNLOAD: usize = 8_000_000;
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
            description: "Fetch a public HTTP(S) URL and return plain text (HTML stripped, line \
structure kept). Use it to read documentation, changelogs, release notes, a README, an API \
reference or a raw file from a repository before relying on how something works — checking beats \
remembering, and your training has a cutoff. Long pages come back a slice at a time; the footer \
gives the offset for the next one. For weather use https://wttr.in/City?format=3, or \
https://wttr.in/?format=3 to auto-detect by IP.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full http:// or https:// URL."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Start this many characters in. Use the offset the previous call's footer gave you to read on."
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

    // Real web results, from whichever engine will answer. DuckDuckGo's
    // instant-answer API returns nothing for most queries (place names, "X
    // weather", and anything technical), so the HTML endpoints are the primary and
    // it is only consulted at the end.
    let mut blocked = 0;
    for engine in [Engine::DuckDuckGo, Engine::Brave] {
        let found = match engine {
            Engine::DuckDuckGo => ddg_html_results(&client, query).await,
            Engine::Brave => brave_results(&client, query).await,
        };
        match found {
            Ok(results) if !results.is_empty() => {
                let mut block = format!("Top web results for \"{query}\" (via {engine}):");
                for (i, r) in results.iter().take(6).enumerate() {
                    block.push_str(&format!("\n{}. {r}", i + 1));
                }
                block.push_str("\n\nweb_fetch any of these urls to read the page itself.");
                return truncate_text(&block, MAX_BODY);
            }
            Err(e) if e == CHALLENGED => blocked += 1,
            _ => {}
        }
    }

    // Fallback: instant answer (definitions, calculations, direct facts).
    if let Some(ia) = ddg_instant_answer(&client, query).await {
        return truncate_text(&ia, MAX_BODY);
    }

    // Say which happened. "No results" when every engine refused to search is a
    // statement about the web that is not true, and it is the kind of untruth that
    // gets repeated to the user as fact.
    if blocked > 0 {
        return format!(
            "error: search is blocked from this machine — {blocked} engine(s) answered with a bot \
             check rather than results. This says nothing about whether \"{query}\" has answers. \
             Use web_fetch on a url directly if you know one, or ask the user to search."
        );
    }

    format!(
        "No results for \"{query}\". The engines answered and had nothing. For weather, web_fetch \
https://wttr.in/CITY?format=3 (URL-encode spaces as +). For a specific site, call web_fetch with \
its URL directly."
    )
}

/// The engines tried, in order.
#[derive(Clone, Copy)]
enum Engine {
    DuckDuckGo,
    Brave,
}

impl std::fmt::Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Engine::DuckDuckGo => "DuckDuckGo",
            Engine::Brave => "Brave",
        })
    }
}

const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// One search result: what it is, where it is, and what it says.
///
/// The url is the point. Results used to come back as title-and-snippet prose with
/// the link thrown away, which severs search from web_fetch — the agent could see
/// that an answer existed and had no way to open it, and guessing URLs from titles
/// is how you end up fetching 404s.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl std::fmt::Display for SearchHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n   {}", self.title, self.url)?;
        if !self.snippet.is_empty() {
            write!(f, "\n   {}", self.snippet)?;
        }
        Ok(())
    }
}

/// Whether a search engine served a bot check instead of results.
///
/// Worth naming separately from "nothing found". They call for opposite things —
/// try another engine, versus accept that the web has no answer — and reporting a
/// block as an empty result set tells the agent something false about the world.
fn is_search_challenge(html: &str) -> bool {
    let lower = html.to_lowercase();
    ["anomaly-modal", "challenge-form", "anomaly.js", "captcha", "confirm this search was made by a human", "detected unusual", "automated queries"]
        .iter()
        .any(|m| lower.contains(m))
}

/// Real search results (title — snippet) scraped from DuckDuckGo's HTML endpoint.
/// Search results from Brave, used when DuckDuckGo answers with a bot check.
///
/// Not a nicety: DDG's HTML endpoint serves a CAPTCHA to datacentre addresses, and
/// a good number of the machines xConsole runs an agent from are datacentre
/// addresses. One engine meant search simply stopped working there, and said
/// "no results" while it did.
async fn brave_results(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .get("https://search.brave.com/search")
        .query(&[("q", query)])
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
    let hits = parse_brave(&html);
    // Order matters: a page with results is a page with results, whatever else is
    // on it. Brave ships captcha wording in its own scripts, so testing for the
    // challenge first would throw away perfectly good results.
    if hits.is_empty() && is_search_challenge(&html) {
        return Err(CHALLENGED.into());
    }
    Ok(hits)
}

/// Pull hits out of a Brave results page.
///
/// Brave's markup is generated, and its class names carry a build hash that will
/// change without warning — so the only things trusted here are the `snippet`
/// container and the first outbound link inside it. Titles are taken as the first
/// substantial line of the block, which is right for most results and occasionally
/// yields the site name instead of the page title; the url and the snippet, which
/// are what the agent acts on, are exact.
fn parse_brave(html: &str) -> Vec<SearchHit> {
    let mut out = Vec::new();
    for block in html.split("<div class=\"snippet").skip(1) {
        // Stop at the next block so one result cannot borrow the next one's link.
        let Some(url) = first_outbound_link(block) else { continue };
        let lines: Vec<String> = html_to_text(block)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| {
                l.chars().count() > 12
                    && !l.contains("svelte-")
                    && !l.starts_with('\u{203a}')
            })
            .collect();
        let snippet = lines
            .iter()
            .max_by_key(|l| l.len())
            .cloned()
            .unwrap_or_default();
        // Brave's block leads with the site name and a breadcrumb before the actual
        // page title. Skip both: a list of results that all say "Stack Overflow" is
        // a list you cannot choose from.
        let host = url
            .split('/')
            .nth(2)
            .unwrap_or("")
            .trim_start_matches("www.")
            .to_string();
        let title = lines
            .iter()
            .find(|l| {
                **l != snippet
                    && !l.contains('\u{203a}')
                    && !l.trim_start_matches("www.").eq_ignore_ascii_case(&host)
                    && l.contains(' ')
            })
            .or_else(|| lines.iter().find(|l| **l != snippet))
            .or_else(|| lines.first())
            .cloned()
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        if out.iter().any(|h: &SearchHit| h.url == url) {
            continue;
        }
        out.push(SearchHit { title, url, snippet });
        if out.len() >= 6 {
            break;
        }
    }
    out
}

/// The first https link in a result block that points somewhere other than the
/// search engine itself (favicons and thumbnails are served from its own domains).
fn first_outbound_link(block: &str) -> Option<String> {
    let mut from = 0;
    while let Some(rel) = block[from..].find("href=\"https://") {
        let start = from + rel + 6;
        let rest = &block[start..];
        let end = rest.find('"')?;
        let url = &rest[..end];
        from = start + end;
        if !url.contains("search.brave.com") && !url.contains("imgs.search.brave") {
            return Some(decode_entities(url));
        }
    }
    None
}

async fn ddg_html_results(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
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

    if is_search_challenge(&html) {
        return Err(CHALLENGED.into());
    }
    Ok(parse_ddg(&html))
}

/// The marker an engine returns when it served a bot check rather than results.
const CHALLENGED: &str = "challenged";

/// Pull hits out of DuckDuckGo's HTML endpoint.
///
/// Separated from the request so the parsing is testable without the network —
/// which matters more than usual here, because the endpoint answers datacentre
/// addresses with a CAPTCHA, and a parser that can only be exercised from a
/// machine that gets real results is a parser that never gets exercised.
fn parse_ddg(html: &str) -> Vec<SearchHit> {
    let titles = anchor_inner_texts(html, "result__a");
    let snippets = anchor_inner_texts(html, "result__snippet");
    let urls = anchor_hrefs(html, "result__a");
    let mut out = Vec::new();
    for i in 0..titles.len() {
        let title = titles.get(i).cloned().unwrap_or_default();
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        let url = urls.get(i).cloned().unwrap_or_default();
        if title.is_empty() && snippet.is_empty() {
            continue;
        }
        out.push(SearchHit { title, url, snippet });
        if out.len() >= 6 {
            break;
        }
    }
    out
}

/// The href of every `<a class="<class>" …>`, unwrapped from DuckDuckGo's redirector.
///
/// DDG hands back `//duckduckgo.com/l/?uddg=<percent-encoded real url>`. Passing
/// that on would be worse than passing nothing: it looks like a usable link right
/// up to the moment web_fetch follows it somewhere else.
fn anchor_hrefs(html: &str, class: &str) -> Vec<String> {
    let needle = format!("class=\"{class}\"");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find(&needle) {
        let cls = from + rel;
        // The href may sit either side of the class inside the same tag, so look
        // from the tag's own opening bracket.
        let open = html[..cls].rfind('<').unwrap_or(cls);
        let Some(gt) = html[cls..].find('>') else { break };
        let tag = &html[open..cls + gt];
        from = cls + gt;
        let Some(h) = tag.find("href=\"") else { continue };
        let rest = &tag[h + 6..];
        let Some(end) = rest.find('"') else { continue };
        if let Some(u) = decode_ddg_href(&rest[..end]) {
            out.push(u);
        }
    }
    out
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
    let text = match fetch_text_full(url_str).await {
        Ok(text) => text,
        Err(e) => return e,
    };
    page_of(&text, args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize)
}

/// One MAX_BODY-sized window of a fetched page, starting at `offset` characters.
///
/// Truncating with no way to ask for the rest makes a long page a dead end: the
/// answer is in the part that was cut, and re-fetching returns the same first
/// slice forever. The footer says how much is left and what offset reaches it, so
/// the next call is obvious rather than something to be worked out.
fn page_of(text: &str, offset: usize) -> String {
    let start = text
        .char_indices()
        .nth(offset)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    if start >= text.len() && offset > 0 {
        return format!("(no more content — the page ends at offset {})", text.chars().count());
    }
    let rest = &text[start..];
    if rest.len() <= MAX_BODY {
        return rest.to_string();
    }
    let window = super::text::truncate_bytes(rest, MAX_BODY);
    let next = offset + window.chars().count();
    let total = text.chars().count();
    format!(
        "{window}\n\n… [{} of {total} characters shown. For the rest call web_fetch again on the \
         same url with offset: {next}]",
        next.min(total)
    )
}

/// Check if an HTTP response represents a CAPTCHA challenge, Cloudflare block, or empty SPA shell.
fn is_challenge_or_empty_shell(status_code: u16, content_type: &str, body: &str) -> bool {
    if status_code == 403 || status_code == 503 || status_code == 429 {
        return true;
    }
    let lower = body.to_lowercase();
    if lower.contains("<title>just a moment...</title>")
        || lower.contains("cf-chl-bypass")
        || lower.contains("cf-challenge")
        || lower.contains("challenge-running")
        || lower.contains("please enable javascript")
        || lower.contains("turn on javascript and cookies")
        || lower.contains("security check to access")
        || lower.contains("checking your browser")
        || lower.contains("hcaptcha")
        || lower.contains("g-recaptcha")
        || lower.contains("datadome")
        || lower.contains("anomaly-modal")
    {
        return true;
    }
    if content_type.contains("html") || body.trim_start().starts_with('<') {
        let text = html_to_text(body);
        let trimmed = text.trim();
        if trimmed.len() < 50
            && (lower.contains("<div id=\"root\"")
                || lower.contains("<div id=\"app\"")
                || lower.contains("<div id=\"__next\"")
                || lower.contains("<main id=\"root\""))
        {
            return true;
        }
    }
    false
}

/// Fallback to Jina Reader AI (https://r.jina.ai/{url}) which renders JS, bypasses anti-bot/CAPTCHA
/// challenges, and extracts clean, rich Markdown for LLM ingestion.
async fn fetch_via_ai_reader(url_str: &str) -> Result<String, String> {
    let jina_url = format!("https://r.jina.ai/{url_str}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .map_err(|e| format!("reader client error: {e}"))?;

    let resp = client
        .get(&jina_url)
        .header(reqwest::header::ACCEPT, "text/plain")
        .header("X-Return-Format", "markdown")
        .header("X-No-Cache", "true")
        .send()
        .await
        .map_err(|e| format!("reader fetch error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("reader HTTP {}", resp.status()));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("reader read body: {e}"))?;

    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("Error:")
        || trimmed.starts_with("AuthenticationRequiredError")
    {
        return Err("reader returned empty or error".into());
    }

    Ok(trimmed.to_string())
}

/// Fetch a public URL and return its plain text (HTML stripped, SSRF-guarded, size-capped).
/// If the direct fetch hits Cloudflare/CAPTCHA bot protection or an empty JS shell, automatically
/// falls back to the Jina AI Reader service for clean markdown extraction.
/// One bounded chunk of a page, for callers that only want a look at the top.
pub async fn fetch_text(url_str: &str) -> Result<String, String> {
    fetch_text_full(url_str).await.map(|t| truncate_text(&t, MAX_BODY))
}

/// The whole page as text, however long it is.
///
/// Split from [`fetch_text`] so `web_fetch` can hand out one window at a time and
/// still reach the end of a long document. Truncating inside the fetch made the
/// tail of a page permanently unreachable — every retry returned the same opening
/// slice, and the answer, if it was further down, could not be got at at all.
pub async fn fetch_text_full(url_str: &str) -> Result<String, String> {
    let url = validate_public_url(url_str)?;
    let client = http_client()?;

    let direct_res = client.get(url.clone()).send().await;

    let (needs_reader, direct_err_msg) = match direct_res {
        Ok(resp) => {
            let status = resp.status();
            let status_u16 = status.as_u16();

            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_lowercase();

            if !status.is_success() {
                if status_u16 == 403 || status_u16 == 503 || status_u16 == 429 {
                    (true, String::new())
                } else {
                    return Err(format!("error: HTTP {} for {url}", status));
                }
            } else {
                match resp.bytes().await {
                    Ok(bytes) => {
                        // Only refuse what would actually hurt to hold in memory. The
                        // old ceiling was four times MAX_BODY *of raw bytes*, which
                        // rejected most real documentation outright: markup, inline CSS
                        // and script are the bulk of a modern page, and a 300KB HTML
                        // file is routinely 15KB of prose. Judging the download by the
                        // size of the text it contains is the wrong way round — strip
                        // it first, then decide what fits.
                        if bytes.len() > MAX_DOWNLOAD {
                            return Err(format!(
                                "error: response too large ({} bytes, max {MAX_DOWNLOAD}). \
                                 Fetch a more specific URL.",
                                bytes.len()
                            ));
                        }
                        let raw = String::from_utf8_lossy(&bytes);
                        if is_challenge_or_empty_shell(status_u16, &content_type, &raw) {
                            (true, String::new())
                        } else {
                            let text = if content_type.contains("html") || raw.trim_start().starts_with('<') {
                                html_to_text(&raw)
                            } else {
                                raw.into_owned()
                            };
                            return Ok(text);
                        }
                    }
                    Err(e) => (true, e.to_string()),
                }
            }
        }
        Err(e) => (true, e.to_string()),
    };

    if needs_reader {
        if let Ok(reader_text) = fetch_via_ai_reader(url_str).await {
            if !reader_text.is_empty() {
                return Ok(reader_text);
            }
        }
    }

    if !direct_err_msg.is_empty() {
        return Err(format!("error: fetch failed: {direct_err_msg}"));
    }

    Err(format!("error: could not fetch {url} (blocked or empty content)"))
}

/// Public wrapper for the search tool — returns the same DuckDuckGo summary block the
/// `web_search` tool produces (titles + snippets), for autoresearch grounding.
pub async fn search_summary(query: &str) -> String {
    web_search(&json!({ "query": query })).await
}

/// Gather research source pages for a topic: run a DuckDuckGo search, extract the top
/// result URLs, and fetch up to `max_fetch` of them. Returns `(url, body_text)` pairs.
/// This is the load-bearing input for skill synthesis — snippets alone are too thin to
/// ground real commands, so the loop reads the actual pages. Best-effort: an empty Vec
/// means search/fetch found nothing usable (the caller degrades gracefully).
pub async fn research_sources(query: &str, max_fetch: usize) -> Vec<(String, String)> {
    let Ok(client) = http_client() else {
        return Vec::new();
    };
    let urls = ddg_result_urls(&client, query).await.unwrap_or_default();
    let mut out: Vec<(String, String)> = Vec::new();
    for url in urls.into_iter() {
        if out.len() >= max_fetch.max(1) {
            break;
        }
        // Each page goes through the same SSRF-guarded fetch as the tool.
        if let Ok(body) = fetch_text(&url).await {
            if !body.trim().is_empty() && !body.starts_with("error:") {
                out.push((url, body));
            }
        }
    }
    out
}

/// Top organic result URLs from DuckDuckGo's HTML endpoint, decoded from its `uddg`
/// redirect wrapper and SSRF-validated. Used by [`research_sources`].
async fn ddg_result_urls(client: &reqwest::Client, query: &str) -> Result<Vec<String>, String> {
    let resp = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("error: search request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("error: search HTTP {}", resp.status()));
    }
    let html = resp.text().await.map_err(|e| e.to_string())?;
    if html.contains("anomaly-modal") || html.contains("challenge-form") {
        return Err("error: search challenge encountered".into());
    }
    Ok(parse_ddg_result_urls(&html))
}

/// Parse + decode organic result URLs from DuckDuckGo HTML (pure, testable). DDG wraps
/// each hit in `<a class="result__a" href="//duckduckgo.com/l/?uddg=<percent-encoded>">`.
fn parse_ddg_result_urls(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let needle = "class=\"result__a\"";
    let mut from = 0;
    while let Some(rel) = html[from..].find(needle) {
        let cls = from + rel;
        // Find the href on this anchor (search backwards a little and forwards to the tag end).
        let tag_start = html[..cls].rfind('<').unwrap_or(cls);
        let tag_end = html[cls..].find('>').map(|g| cls + g).unwrap_or(html.len());
        let tag = &html[tag_start..tag_end];
        if let Some(href) = extract_attr(tag, "href") {
            if let Some(decoded) = decode_ddg_href(&href) {
                if validate_public_url(&decoded).is_ok() && !out.contains(&decoded) {
                    out.push(decoded);
                }
            }
        }
        from = tag_end.max(cls + needle.len());
    }
    out
}

/// Value of an HTML attribute (`name="value"`) within a single tag string.
fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// Decode DuckDuckGo's `/l/?uddg=<url>` redirect wrapper into the real target URL.
/// Also accepts already-absolute hrefs. Returns None for non-result links.
fn decode_ddg_href(href: &str) -> Option<String> {
    let h = decode_entities(href);
    // Wrapped form: //duckduckgo.com/l/?uddg=<percent-encoded>&rut=…
    if let Some(idx) = h.find("uddg=") {
        let rest = &h[idx + 5..];
        let enc = rest.split('&').next().unwrap_or(rest);
        let dec = percent_decode(enc);
        if dec.starts_with("http://") || dec.starts_with("https://") {
            return Some(dec);
        }
    }
    // Already-absolute (some layouts): take as-is.
    if h.starts_with("http://") || h.starts_with("https://") {
        return Some(h);
    }
    None
}

/// Minimal percent-decoder (no extra crate) for the `uddg` query value.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn http_client() -> Result<reqwest::Client, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(BROWSER_UA),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        ),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        reqwest::header::HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-ch-ua"),
        reqwest::header::HeaderValue::from_static(
            "\"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\"",
        ),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-ch-ua-mobile"),
        reqwest::header::HeaderValue::from_static("?0"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-ch-ua-platform"),
        reqwest::header::HeaderValue::from_static("\"Windows\""),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-dest"),
        reqwest::header::HeaderValue::from_static("document"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-mode"),
        reqwest::header::HeaderValue::from_static("navigate"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-site"),
        reqwest::header::HeaderValue::from_static("none"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-user"),
        reqwest::header::HeaderValue::from_static("?1"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("upgrade-insecure-requests"),
        reqwest::header::HeaderValue::from_static("1"),
    );

    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .default_headers(headers)
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
    // `host_str()` serializes IPv6 hosts WITH brackets (e.g. "[fe80::1]"), which
    // `IpAddr::parse` rejects — strip them so IPv6 literals get classified too.
    // Without this, the private/metadata guard is trivially bypassed via IPv6
    // (including IPv4-mapped forms like [::ffff:169.254.169.254]).
    let ip_str = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host.as_str());
    if let Ok(ip) = ip_str.parse::<IpAddr>() {
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

/// Public wrapper so the autoresearch query sanitizer can reuse the same
/// private/reserved-IP classification used by the SSRF guard.
pub fn is_private_ip_pub(ip: IpAddr) -> bool {
    is_private_ip(ip)
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

/// Whether a tag ends the line it is on.
///
/// Opening *and* closing forms both count. Only `<p>` and `<div>` opens used to,
/// which left every list, heading and table row concatenated into its neighbours
/// — a documentation sidebar came back as one unreadable run of words like
/// "VecSectionsExamplesIndexing". A closing `</li>` is exactly where a line ends.
fn breaks_line(tag: &str) -> bool {
    const BLOCK: &[&str] = &[
        "br", "p", "div", "li", "ul", "ol", "tr", "table", "section", "article", "header",
        "footer", "nav", "aside", "blockquote", "pre", "hr", "h1", "h2", "h3", "h4", "h5", "h6",
        "dt", "dd", "dl", "form", "fieldset", "figure", "figcaption", "main", "details",
        "summary", "option",
    ];
    let name = tag.trim_start_matches('/');
    // Match on the tag name only: `div class="x"` and `p` alike, but never `param`
    // for `p` or `path` for `p`.
    let name = name
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("");
    BLOCK.contains(&name)
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
                } else if breaks_line(&tag) && !out.ends_with('\n') {
                    // Open and close both break, so guard against the pair emitting
                    // two: one blank line between every block is the whole page
                    // twice as long for nothing.
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

    // Collapse runs of spaces inside a line but keep the line breaks the tags above
    // just put in. Flattening everything to one paragraph — which is what this did —
    // destroys exactly the structure that makes a page readable: headings, list
    // items, and the line breaks in a code sample all become one wall of words.
    let decoded = decode_entities(&out);
    let mut lines: Vec<&str> = Vec::new();
    for line in decoded.lines() {
        let t = line.trim();
        // At most one blank line in a row: HTML is full of empty wrappers, and a
        // page of blank lines is as useless as a page with none.
        if t.is_empty() && lines.last().map(|l: &&str| l.is_empty()).unwrap_or(true) {
            continue;
        }
        lines.push(t);
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
        .iter()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
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
    fn a_fetched_page_keeps_its_lines() {
        // The parser inserts breaks for <p>/<br>/<div> and the old last line threw
        // every one of them away, so a page came back as one wall of words —
        // headings, list items and code samples all run together.
        let html = "<h1>Install</h1><p>Run this:</p><div>npm i foo</div><div>npm test</div>";
        let text = html_to_text(html);
        assert!(text.contains("npm i foo\nnpm test"), "lines were flattened: {text:?}");
    }

    #[test]
    fn a_fetched_page_is_readable_rather_than_escaped() {
        // Reading a code sample full of &lt;div&gt; and &amp;&amp; is reading it
        // wrong, and it is the samples people fetch documentation for.
        let text = html_to_text("<p>use &lt;T&gt; where T: A &amp;&amp; B</p>");
        assert!(text.contains("use <T> where T: A && B"), "{text:?}");
    }

    #[test]
    fn blank_lines_do_not_pile_up() {
        let text = html_to_text("<div></div><div></div><div>a</div><div></div><div></div><div>b</div>");
        assert!(!text.contains("\n\n\n"), "empty wrappers became a page of blank lines: {text:?}");
        assert!(text.contains('a') && text.contains('b'));
    }

    #[test]
    fn a_long_page_can_be_read_past_the_first_window() {
        // Truncating with no way to ask for more makes the tail of a document
        // unreachable: every retry hands back the same opening slice.
        // Every line distinct, so "the second window is not the first" is a claim
        // about the paging and not about the shape of the fixture.
        let text: String = (0..MAX_BODY / 4)
            .map(|i| format!("line {i} of the document\n"))
            .collect();
        let first = page_of(&text, 0);
        assert!(first.contains("line 0 of"), "the first window did not start at the top");
        assert!(first.contains("For the rest call web_fetch again"), "no way onward");
        let next: usize = first
            .rsplit("offset: ")
            .next()
            .and_then(|t| t.trim_end_matches(']').trim().parse().ok())
            .expect("the footer names the next offset");
        assert!(next > 0 && next < text.chars().count());
        let second = page_of(&text, next);
        assert!(
            !second.contains("line 0 of the document"),
            "the second window started back at the top instead of where the first ended"
        );
        assert!(
            second.contains(&format!("line {} of", next / 24)) || second.len() > 100,
            "the second window came back empty"
        );
        assert!(page_of(&text, text.chars().count() + 1).contains("no more content"));
    }

    #[test]
    fn a_page_that_fits_comes_back_whole_and_unadorned() {
        let text = "short and complete";
        assert_eq!(page_of(text, 0), text, "a small page grew a paging footer");
    }

    #[test]
    fn an_ordinary_documentation_page_is_not_refused_for_its_size() {
        // The old ceiling was 4x MAX_BODY of *raw bytes*. Markup, inline CSS and
        // script are most of a modern page, so real docs were rejected unread.
        assert!(
            MAX_DOWNLOAD > MAX_BODY * 20,
            "the download guard is still sized as if markup were content"
        );
    }

    #[test]
    fn a_search_result_carries_the_link_not_just_the_prose() {
        // Results used to be title-and-snippet with the url discarded, which cuts
        // search off from web_fetch: the agent can see an answer exists and has no
        // way to open it.
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio%2Flatest%2F&amp;rut=x">spawn_blocking in tokio</a>
            <a class="result__snippet" href="//y">Runs the provided closure on a thread.</a>"#;
        let hits = parse_ddg(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://docs.rs/tokio/latest/");
        assert!(hits[0].title.contains("spawn_blocking"));
        assert!(hits[0].snippet.contains("closure"));
        assert!(
            format!("{}", hits[0]).contains("https://docs.rs/tokio/latest/"),
            "the url is parsed but not shown to the agent"
        );
    }

    #[test]
    fn a_redirect_wrapper_is_never_passed_off_as_a_real_link() {
        // duckduckgo.com/l/?uddg=… looks like a usable url right up to the point
        // web_fetch follows it somewhere else.
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%20b">T</a>"#;
        let urls = anchor_hrefs(html, "result__a");
        assert_eq!(urls, vec!["https://example.com/a b"]);
    }

    #[test]
    fn a_bot_check_is_not_reported_as_an_empty_web() {
        // These call for opposite responses — try another engine, versus accept
        // that there is no answer — so telling them apart is the whole point.
        assert!(is_search_challenge(
            "<div>Please complete the following challenge to confirm this search was made by a human</div>"
        ));
        assert!(is_search_challenge("<div id=\"anomaly-modal\">"));
        assert!(is_search_challenge("your network appears to be sending automated queries"));
        assert!(!is_search_challenge(
            "<a class=\"result__a\" href=\"//x\">An ordinary page about ducks</a>"
        ));
    }

    #[test]
    fn brave_results_are_read_out_of_the_generated_markup() {
        // The class names carry a build hash, so only the container and the first
        // outbound link are trusted. This is the shape the live page has.
        let html = r#"<div class="snippet svelte-jmfu5f" data-pos="0" data-type="web">
            <a href="https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html" class="svelte-14r20fy l1">
            <img src="https://imgs.search.brave.com/abc" alt=""/>
            <div>spawn_blocking in tokio::task - Rust</div>
            <div>This function runs the provided closure on a dedicated thread pool.</div>
            </a></div>"#;
        let hits = parse_brave(html);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(
            hits[0].url,
            "https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html"
        );
        assert!(hits[0].snippet.contains("dedicated thread pool"));
    }

    #[test]
    fn the_engines_own_images_are_not_mistaken_for_results() {
        let block = r#"<a href="https://imgs.search.brave.com/favicon"></a>
                       <a href="https://search.brave.com/settings"></a>
                       <a href="https://real-result.example/page"></a>"#;
        assert_eq!(
            first_outbound_link(block).as_deref(),
            Some("https://real-result.example/page")
        );
    }

    #[test]
    fn one_result_cannot_borrow_the_next_ones_link() {
        let html = r#"<div class="snippet a"><a href="https://first.example/">First result title here</a></div>
                      <div class="snippet b"><a href="https://second.example/">Second result title here</a></div>"#;
        let hits = parse_brave(html);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].url, "https://first.example/");
        assert_eq!(hits[1].url, "https://second.example/");
    }

    #[test]
    fn truncate_text_handles_multibyte() {
        let s = "ä".repeat(100); // 200 bytes
        // Must not panic slicing mid-codepoint, and must stay within budget.
        let out = truncate_text(&s, 51);
        assert!(out.starts_with('ä'));
    }
}

#[cfg(test)]
mod live_fetch_smoke {
    /// Not part of the suite — a real fetch, run by hand with
    /// `cargo test --lib live_fetch -- --ignored --nocapture` to see what a page
    /// actually looks like after stripping. Unit tests prove the shape; this
    /// proves the shape matches the web.
    /// The whole search chain against the live web.
    #[tokio::test]
    #[ignore]
    async fn a_real_search_returns_links_worth_following() {
        let out = super::web_search(&serde_json::json!({"query": "rust tokio spawn_blocking"})).await;
        println!("{out}");
        assert!(
            out.contains("https://"),
            "search came back with no url to follow: {out}"
        );
        assert!(
            !out.starts_with("No results"),
            "an engine was blocked and it was reported as an empty web"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn a_real_documentation_page_comes_back_readable() {
        let text = super::fetch_text_full("https://doc.rust-lang.org/std/vec/struct.Vec.html")
            .await
            .expect("fetch works");
        println!(
            "chars={} lines={}",
            text.chars().count(),
            text.lines().count()
        );
        println!("--- head ---\n{}", super::super::text::truncate_bytes(&text, 500));
        assert!(text.lines().count() > 20, "still one flat wall of words");
    }
}

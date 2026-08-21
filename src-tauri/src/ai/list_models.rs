//! Model listing for cloud providers — the "autodetect models" feature.
//!
//! OpenAI-compatible providers expose `GET {base}/models` (Bearer); Anthropic exposes
//! `GET {base}/v1/models?limit=100` (x-api-key). On any failure we return an empty
//! list and the frontend falls back to the curated catalog models.

use serde_json::Value;

/// Normalise a base URL so `/models` lands right: strip a trailing slash, and if the
/// URL is Anthropic-style (`https://api.anthropic.com` without a path) the caller
/// passes the full path in `suffix`.
fn join(base: &str, suffix: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        suffix.to_string()
    } else {
        format!("{base}/{suffix}")
    }
}

/// Fetch models from an OpenAI-compatible `/models` endpoint.
pub async fn list_openai_compatible(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let url = join(base_url, "models");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GET {url} -> {}", resp.status()));
    }
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    // Accept every shape rick's ParseModels handles: {data:[{id}]}, {data:[{type}]},
    // {models:[...]}, or a bare array.
    let ids = extract_ids(&body);
    Ok(ids)
}

/// Fetch models from an Anthropic `/v1/models?limit=100` endpoint.
pub async fn list_anthropic(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let url = join(base_url, "v1/models?limit=100");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GET {url} -> {}", resp.status()));
    }
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(extract_ids(&body))
}

/// Fetch models from a local Ollama `/api/tags` endpoint.
pub async fn list_ollama(base_url: &str) -> Result<Vec<String>, String> {
    let url = join(base_url, "api/tags");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GET {url} -> {}", resp.status()));
    }
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Some(models) = body.get("models").and_then(|m| m.as_array()) {
        for item in models {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                out.push(name.to_string());
            } else if let Some(name) = item.get("model").and_then(|v| v.as_str()) {
                out.push(name.to_string());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    Ok(out
        .into_iter()
        .filter(|id| !id.trim().is_empty())
        .filter(|id| seen.insert(id.clone()))
        .collect())
}

/// Pull model ids out of any of the common response shapes.
fn extract_ids(body: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(models) = body.get("models").and_then(|m| m.as_array()) {
        for item in models {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_string());
            }
        }
    } else if let Some(arr) = body.as_array() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_string());
            }
        }
    }
    // Dedup, preserve order, skip empty.
    let mut seen = std::collections::HashSet::new();
    out.into_iter()
        .filter(|id| !id.trim().is_empty())
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_ids_handles_openai_shape() {
        let body = json!({
            "object": "list",
            "data": [
                {"id": "gpt-5", "object": "model"},
                {"id": "gpt-4o", "object": "model"},
            ]
        });
        assert_eq!(extract_ids(&body), vec!["gpt-5", "gpt-4o"]);
    }

    #[test]
    fn extract_ids_handles_anthropic_shape() {
        let body = json!({
            "data": [
                {"type": "model", "id": "claude-sonnet-4-5"},
                {"type": "model", "id": "claude-opus-4-5"},
            ]
        });
        assert_eq!(extract_ids(&body), vec!["claude-sonnet-4-5", "claude-opus-4-5"]);
    }

    #[test]
    fn extract_ids_handles_bare_array_and_dedups() {
        let body = json!([
            {"id": "a"},
            {"id": "b"},
            {"id": "a"},
        ]);
        assert_eq!(extract_ids(&body), vec!["a", "b"]);
    }

    #[test]
    fn join_normalises_trailing_slash() {
        assert_eq!(join("https://api.x.ai/v1", "models"), "https://api.x.ai/v1/models");
        assert_eq!(join("https://api.x.ai/v1/", "models"), "https://api.x.ai/v1/models");
        assert_eq!(join("https://api.anthropic.com", "v1/models?limit=100"), "https://api.anthropic.com/v1/models?limit=100");
        assert_eq!(join("http://localhost:11434/", "api/tags"), "http://localhost:11434/api/tags");
    }
}

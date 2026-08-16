//! Multi-provider image generation engine.
//! Supports Pollinations (Flux) as a zero-config free tier, plus OpenAI DALL-E 3
//! when an API key is configured.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenResult {
    pub path: String,
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub provider: String,
    pub markdown: String,
}

/// Calculate dimensions from aspect ratio string.
pub fn dimensions_from_aspect(aspect_ratio: &str) -> (u32, u32) {
    match aspect_ratio.trim() {
        "16:9" => (1280, 720),
        "9:16" => (720, 1280),
        "4:3" => (1024, 768),
        "3:4" => (768, 1024),
        "3:2" => (1080, 720),
        "2:3" => (720, 1080),
        _ => (1024, 1024), // 1:1 default
    }
}

/// Simple zero-dependency URL encoder for prompt strings.
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push_str("%20"),
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// Generate an image using Pollinations.ai (Flux model) without requiring API keys.
pub async fn generate_pollinations(
    prompt: &str,
    width: u32,
    height: u32,
    dest_dir: &Path,
    name_hint: Option<&str>,
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(dest_dir)
        .await
        .map_err(|e| format!("failed to create image destination directory: {e}"))?;

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42);
    let encoded_prompt = url_encode(prompt);
    let url = format!(
        "https://image.pollinations.ai/prompt/{encoded_prompt}?width={width}&height={height}&seed={seed}&model=flux&nologo=true"
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_default();

    let resp = client
        .get(&url)
        .header("user-agent", "xConsole/1.0 (Desktop)")
        .send()
        .await
        .map_err(|e| format!("image generation request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("pollinations error {status}: {body}"));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("failed to read image response bytes: {e}"))?;

    let filename = if let Some(hint) = name_hint.filter(|h| !h.trim().is_empty()) {
        let clean: String = hint
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        format!("{clean}_{}.png", &Uuid::new_v4().to_string()[..8])
    } else {
        format!("img_{}.png", &Uuid::new_v4().to_string()[..8])
    };

    let out_path = dest_dir.join(filename);
    tokio::fs::write(&out_path, &bytes)
        .await
        .map_err(|e| format!("failed to save generated image file: {e}"))?;

    Ok(out_path)
}

/// Generate an image using OpenAI DALL-E 3.
pub async fn generate_openai(
    api_key: &str,
    prompt: &str,
    width: u32,
    height: u32,
    dest_dir: &Path,
    name_hint: Option<&str>,
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(dest_dir)
        .await
        .map_err(|e| format!("failed to create image destination directory: {e}"))?;

    let size_str = if width > height {
        "1792x1024"
    } else if height > width {
        "1024x1792"
    } else {
        "1024x1024"
    };

    let body = json!({
        "model": "dall-e-3",
        "prompt": prompt,
        "n": 1,
        "size": size_str,
        "response_format": "b64_json"
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .unwrap_or_default();

    let resp = client
        .post("https://api.openai.com/v1/images/generations")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenAI DALL-E request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("OpenAI DALL-E error {status}: {text}"));
    }

    let json_resp: Value = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse DALL-E response JSON: {e}"))?;

    let b64 = json_resp["data"][0]["b64_json"]
        .as_str()
        .ok_or_else(|| "missing b64_json in OpenAI response".to_string())?;

    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("failed to decode base64 image: {e}"))?;

    let filename = if let Some(hint) = name_hint.filter(|h| !h.trim().is_empty()) {
        let clean: String = hint
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        format!("{clean}_{}.png", &Uuid::new_v4().to_string()[..8])
    } else {
        format!("dalle_{}.png", &Uuid::new_v4().to_string()[..8])
    };

    let out_path = dest_dir.join(filename);
    tokio::fs::write(&out_path, &bytes)
        .await
        .map_err(|e| format!("failed to save DALL-E image file: {e}"))?;

    Ok(out_path)
}

//! Image vision: native multimodal blocks vs a side-call `vision` tool.
//!
//! Session model never switches. If it can see pixels, the last user turn carries
//! image blocks. Otherwise the model calls `vision`, which hits a separately
//! configured vision provider (Gemini by default — currently the strongest cheap
//! pair). Only the latest user turn's images are readable. Fail open.

use serde_json::{json, Value};

use crate::ai::context::is_runtime_message;
use crate::ai::provider::{ChatImage, ChatMessage, ChatRequest, ToolDef};
use crate::ai::registry;
use crate::storage::models::AiProvider;
use crate::storage::Db;

pub const SETTING_MODE: &str = "agent.vision_mode";
pub const SETTING_PROVIDER: &str = "agent.vision_provider";
pub const SETTING_MODEL: &str = "agent.vision_model";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionMode {
    /// Prompt once before sending pixels (Command Code default).
    Ask,
    Enabled,
    Disabled,
}

impl VisionMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "enabled" | "on" | "true" | "1" => Self::Enabled,
            "disabled" | "off" | "false" | "0" => Self::Disabled,
            _ => Self::Ask,
        }
    }
}

pub fn mode_from_db(db: &Db) -> VisionMode {
    let raw = db
        .get_setting(SETTING_MODE)
        .ok()
        .flatten()
        .unwrap_or_default();
    VisionMode::parse(&raw)
}

/// Pull images off the latest real user turn and wipe every message so older
/// pixels never ride into the provider prefix.
pub fn take_latest_user_images(messages: &mut [ChatMessage]) -> Vec<ChatImage> {
    let mut images = Vec::new();
    if let Some(m) = messages
        .iter_mut()
        .rev()
        .find(|m| m.role == "user" && !is_runtime_message(m))
    {
        images = std::mem::take(&mut m.images);
    }
    for m in messages.iter_mut() {
        m.images.clear();
    }
    images
}

pub fn strip_all_images(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    for m in &mut messages {
        m.images.clear();
    }
    messages
}

pub fn attach_images_to_latest_user(messages: &mut [ChatMessage], images: Vec<ChatImage>) {
    if images.is_empty() {
        return;
    }
    if let Some(m) = messages
        .iter_mut()
        .rev()
        .find(|m| m.role == "user" && !is_runtime_message(m))
    {
        m.images = images;
    }
}

pub fn model_has_native_vision(kind: &str, model: &str, base_url: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    let m = model.to_ascii_lowercase();
    let url = base_url.to_ascii_lowercase();
    if k == "anthropic" {
        return true;
    }
    if is_gemini_endpoint(&url) || m.contains("gemini") {
        return true;
    }
    // CLI harnesses own their own tools — we never send raw image blocks there.
    if matches!(
        k.as_str(),
        "cursor" | "codex_cli" | "opencode_cli" | "antigravity_cli"
    ) {
        return false;
    }
    const HINTS: &[&str] = &[
        "gpt-4o",
        "gpt-4.1",
        "gpt-4-turbo",
        "gpt-4-vision",
        "gpt-5",
        "chatgpt-4o",
        "o1",
        "o3",
        "o4",
        "claude",
        "grok-2",
        "grok-3",
        "grok-4",
        "qwen-vl",
        "qwen2-vl",
        "qwen2.5-vl",
        "qwen3-vl",
        "pixtral",
        "llama-4",
        "llama4",
        "glm-4v",
        "glm-4.5v",
        "glm-4.6v",
        "mistral-small",
        "mistral-medium",
    ];
    HINTS.iter().any(|h| m.contains(h))
}

pub fn is_gemini_provider(p: &AiProvider) -> bool {
    let name = p.name.to_ascii_lowercase();
    let url = p.base_url.as_deref().unwrap_or("").to_ascii_lowercase();
    name.contains("gemini") || is_gemini_endpoint(&url)
}

fn is_gemini_endpoint(url: &str) -> bool {
    url.contains("generativelanguage.googleapis.com") || url.contains("googleapis.com/v1beta/openai")
}

/// Use native image blocks only when the *session* model can see and the user
/// did not pick a different vision provider/model.
pub fn use_native(
    session_kind: &str,
    session_model: &str,
    session_base_url: &str,
    session_provider_id: &str,
    vision_provider_id: &str,
    vision_model: &str,
    has_images: bool,
) -> bool {
    if !has_images {
        return false;
    }
    if !model_has_native_vision(session_kind, session_model, session_base_url) {
        return false;
    }
    let vision_id = vision_provider_id.trim();
    if !vision_id.is_empty() && vision_id != session_provider_id {
        return false;
    }
    let vision_m = vision_model.trim();
    if !vision_m.is_empty() && vision_m != session_model {
        return false;
    }
    true
}

pub fn tool_def() -> ToolDef {
    ToolDef {
        name: "vision".into(),
        description: "Look at an image the user attached on their latest message \
([Image #1], [Image #2], …). The session model cannot see pixels — call this \
with the 1-based image index and a focused question. Only the latest message's \
images are available."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "image": {
                    "type": "integer",
                    "description": "1-based index matching [Image #n] on the latest user message."
                },
                "question": {
                    "type": "string",
                    "description": "What to look for or describe in the image."
                }
            },
            "required": ["image", "question"]
        }),
    }
}

pub fn tool_hint(count: usize) -> String {
    format!(
        "# Images\nThe user attached {count} image(s) tagged [Image #1]…[Image #{count}]. \
You cannot see the pixels. Call the `vision` tool with the image index and a \
specific question (layout, text, errors, UI, code in a screenshot). Only the \
latest message's images are readable."
    )
}

/// Pick the vision backend. Explicit setting wins; otherwise Gemini if the user
/// has a key configured; otherwise any vision-capable cloud provider.
pub fn pick_vision_provider<'a>(
    providers: &'a [AiProvider],
    preferred_id: &str,
) -> Option<&'a AiProvider> {
    let enabled: Vec<&AiProvider> = providers.iter().filter(|p| p.enabled).collect();
    if enabled.is_empty() {
        return None;
    }
    let pref = preferred_id.trim();
    if !pref.is_empty() {
        if let Some(p) = enabled.iter().find(|p| p.id == pref) {
            return Some(*p);
        }
    }
    if let Some(p) = enabled.iter().find(|p| is_gemini_provider(p)) {
        return Some(*p);
    }
    enabled.iter().copied().find(|p| {
        model_has_native_vision(
            &p.kind,
            p.model.as_deref().unwrap_or(""),
            p.base_url.as_deref().unwrap_or(""),
        )
    }).or_else(|| {
        enabled
            .into_iter()
            .find(|p| registry::is_tool_capable_kind(&p.kind))
    })
}

pub fn default_vision_model(provider: &AiProvider, override_model: &str) -> String {
    let over = override_model.trim();
    if !over.is_empty() {
        return over.to_string();
    }
    if is_gemini_provider(provider) {
        return "gemini-2.5-flash".into();
    }
    provider
        .model
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "gpt-4o-mini".into())
}

pub fn openai_user_content(text: &str, images: &[ChatImage]) -> Value {
    if images.is_empty() {
        return json!(text);
    }
    let mut parts: Vec<Value> = Vec::with_capacity(images.len() + 1);
    if !text.is_empty() {
        parts.push(json!({"type": "text", "text": text}));
    }
    for img in images {
        let mime = sanitize_mime(&img.media_type);
        parts.push(json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{mime};base64,{}", img.data)
            }
        }));
    }
    if parts.is_empty() {
        json!(text)
    } else {
        Value::Array(parts)
    }
}

/// Claude wants images before the text that refers to them.
pub fn anthropic_user_content(text: &str, images: &[ChatImage]) -> Value {
    if images.is_empty() {
        return json!(text);
    }
    let mut parts: Vec<Value> = Vec::with_capacity(images.len() + 1);
    for img in images {
        parts.push(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": sanitize_mime(&img.media_type),
                "data": img.data,
            }
        }));
    }
    if !text.is_empty() {
        parts.push(json!({"type": "text", "text": text}));
    }
    Value::Array(parts)
}

pub fn ollama_image_payloads(images: &[ChatImage]) -> Vec<String> {
    images.iter().map(|i| i.data.clone()).collect()
}

pub fn sanitize_mime(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" | "jpeg" | "jpg" => "image/jpeg",
        "image/gif" | "gif" => "image/gif",
        "image/webp" | "webp" => "image/webp",
        _ => "image/png",
    }
}

pub async fn describe_one(
    db: &Db,
    image: &ChatImage,
    question: &str,
) -> Result<String, String> {
    let (provider, model) = resolve_vision_backend(db)?;
    let q = question.trim();
    let q = if q.is_empty() {
        "Describe this image for a coding agent. Transcribe visible text. Note UI, errors, code, and layout."
    } else {
        q
    };
    let mut msg = ChatMessage::user(q);
    msg.images = vec![image.clone()];
    let mut req = ChatRequest::new(model);
    req.messages = vec![msg];
    req.max_tokens = 2048;
    req.temperature = 0.2;
    let resp = provider.provider.chat(&req, None).await?;
    let text = resp.content.trim();
    if text.is_empty() {
        return Err("vision model returned an empty description".into());
    }
    Ok(resp.content)
}

pub async fn describe_all(db: &Db, images: &[ChatImage], question: &str) -> Result<String, String> {
    if images.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    for (i, img) in images.iter().enumerate() {
        let label = i + 1;
        match describe_one(db, img, question).await {
            Ok(text) => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&format!("[Image #{label} description]\n{text}"));
            }
            Err(e) => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&format!("[Image #{label}] vision failed: {e}"));
            }
        }
    }
    Ok(out)
}

pub fn resolve_vision_backend(db: &Db) -> Result<(registry::ResolvedProvider, String), String> {
    let providers = db.list_providers().map_err(|e| e.to_string())?;
    let preferred = db
        .get_setting(SETTING_PROVIDER)
        .ok()
        .flatten()
        .unwrap_or_default();
    let model_over = db
        .get_setting(SETTING_MODEL)
        .ok()
        .flatten()
        .unwrap_or_default();
    let picked = pick_vision_provider(&providers, &preferred).ok_or_else(|| {
        "no vision model configured — add a Gemini (or other vision) API key in Settings → Providers, then pick it with /vision".to_string()
    })?;
    let model = default_vision_model(picked, &model_over);
    if model.is_empty() {
        return Err("vision provider has no model — set one with /vision".into());
    }
    let resolved = registry::build(db, &picked.id)?;
    Ok((resolved, model))
}

pub fn lookup_turn_image<'a>(images: &'a [ChatImage], index: i64) -> Result<&'a ChatImage, String> {
    if images.is_empty() {
        return Err(
            "no images on the latest user message (only the most recent turn is readable)".into(),
        );
    }
    if index < 1 || index as usize > images.len() {
        return Err(format!(
            "image index {index} is out of range (1–{})",
            images.len()
        ));
    }
    Ok(&images[(index as usize) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(name: &str) -> ChatImage {
        ChatImage {
            media_type: "image/png".into(),
            data: "AAAA".into(),
            name: name.into(),
        }
    }

    fn provider(id: &str, name: &str, kind: &str, url: &str, model: &str) -> AiProvider {
        AiProvider {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            model: Some(model.into()),
            base_url: Some(url.into()),
            bin_path: None,
            extra_json: None,
            enabled: true,
            has_secret: true,
            created_at: None,
        }
    }

    #[test]
    fn mode_defaults_to_ask() {
        assert_eq!(VisionMode::parse(""), VisionMode::Ask);
        assert_eq!(VisionMode::parse("ask"), VisionMode::Ask);
        assert_eq!(VisionMode::parse("enabled"), VisionMode::Enabled);
        assert_eq!(VisionMode::parse("off"), VisionMode::Disabled);
    }

    #[test]
    fn claude_and_gemini_see_pixels() {
        assert!(model_has_native_vision("anthropic", "claude-sonnet-4-5", ""));
        assert!(model_has_native_vision(
            "openai",
            "gemini-2.5-flash",
            "https://generativelanguage.googleapis.com/v1beta/openai"
        ));
        assert!(model_has_native_vision("openai", "gpt-4o", "https://api.openai.com/v1"));
        assert!(!model_has_native_vision(
            "openai",
            "deepseek-chat",
            "https://api.deepseek.com/v1"
        ));
        assert!(!model_has_native_vision("cursor", "auto", ""));
    }

    #[test]
    fn native_skipped_when_user_picks_other_vision_model() {
        assert!(use_native(
            "anthropic",
            "claude-sonnet-4-5",
            "",
            "claude",
            "",
            "",
            true
        ));
        assert!(!use_native(
            "anthropic",
            "claude-sonnet-4-5",
            "",
            "claude",
            "gemini-id",
            "gemini-2.5-flash",
            true
        ));
        assert!(!use_native(
            "openai",
            "deepseek-chat",
            "https://api.deepseek.com/v1",
            "ds",
            "",
            "",
            true
        ));
        assert!(!use_native("anthropic", "claude-sonnet-4-5", "", "claude", "", "", false));
    }

    #[test]
    fn latest_images_only_and_older_stripped() {
        let mut msgs = vec![
            {
                let mut m = ChatMessage::user("old");
                m.images = vec![img("old.png")];
                m
            },
            ChatMessage::assistant("ok"),
            {
                let mut m = ChatMessage::user("new");
                m.images = vec![img("a.png"), img("b.png")];
                m
            },
        ];
        let got = take_latest_user_images(&mut msgs);
        assert_eq!(got.len(), 2);
        assert!(msgs.iter().all(|m| m.images.is_empty()));
    }

    #[test]
    fn gemini_preferred_over_other_providers() {
        let list = vec![
            provider(
                "ds",
                "DeepSeek",
                "openai",
                "https://api.deepseek.com/v1",
                "deepseek-chat",
            ),
            provider(
                "gem",
                "Google Gemini",
                "openai",
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "gemini-2.5-pro",
            ),
            provider(
                "oa",
                "OpenAI",
                "openai",
                "https://api.openai.com/v1",
                "gpt-4o",
            ),
        ];
        let picked = pick_vision_provider(&list, "").unwrap();
        assert_eq!(picked.id, "gem");
        assert_eq!(default_vision_model(picked, ""), "gemini-2.5-flash");
        assert_eq!(pick_vision_provider(&list, "oa").unwrap().id, "oa");
    }

    #[test]
    fn anthropic_puts_images_before_text() {
        let v = anthropic_user_content("see [Image #1]", &[img("shot.png")]);
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["type"], "image");
        assert_eq!(arr[1]["type"], "text");
        assert_eq!(arr[1]["text"], "see [Image #1]");
    }

    #[test]
    fn openai_uses_data_url_parts() {
        let v = openai_user_content("hi", &[img("x.png")]);
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[1]["type"], "image_url");
        assert!(arr[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn text_only_stays_a_string() {
        assert_eq!(openai_user_content("hello", &[]), json!("hello"));
        assert_eq!(anthropic_user_content("hello", &[]), json!("hello"));
    }

    #[test]
    fn image_index_is_one_based() {
        let imgs = vec![img("a"), img("b")];
        assert_eq!(lookup_turn_image(&imgs, 2).unwrap().name, "b");
        assert!(lookup_turn_image(&imgs, 0).is_err());
        assert!(lookup_turn_image(&imgs, 3).is_err());
        assert!(lookup_turn_image(&[], 1).is_err());
    }
}

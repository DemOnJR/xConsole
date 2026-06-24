//! Build a live `Provider` from stored config + keychain secret. This is the one
//! place that maps a provider `kind` to an implementation.

use crate::ai::provider::Provider;
use crate::ai::providers::{
    anthropic::AnthropicProvider, cli::CliProvider, ollama::{parse_ollama_extra, OllamaProvider},
    openai_compat::OpenAiProvider,
};
use crate::secrets;
use crate::storage::Db;

/// Provider kinds that support xConsole's tool loop (SSH, files, infra tools).
pub fn is_tool_capable_kind(kind: &str) -> bool {
    matches!(kind, "openai" | "anthropic" | "ollama" | "llamacpp")
}

/// A constructed provider plus the model it should use.
pub struct ResolvedProvider {
    pub provider: Box<dyn Provider>,
    pub model: String,
    pub name: String,
    pub kind: String,
    /// Ollama `num_ctx` for this provider (None for non-ollama). Read from the
    /// *resolved* provider so context budgeting stays correct on CLI→Ollama fallback.
    pub ollama_num_ctx: Option<u32>,
}

/// Resolve the provider id the agent should use: explicit override, else the
/// configured active provider, else the first enabled one.
pub fn active_provider_id(db: &Db, override_id: Option<&str>) -> Result<String, String> {
    if let Some(id) = override_id {
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }
    if let Some(id) = db.get_setting("agent.active_provider").map_err(|e| e.to_string())? {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let providers = db.list_providers().map_err(|e| e.to_string())?;
    providers
        .into_iter()
        .find(|p| p.enabled)
        .map(|p| p.id)
        .ok_or_else(|| "no AI provider configured".to_string())
}

/// Build a ready-to-use provider for the given provider id.
pub fn build(db: &Db, provider_id: &str) -> Result<ResolvedProvider, String> {
    let p = db
        .get_provider(provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "provider not found".to_string())?;

    let secret = secrets::get_secret(&secrets::provider_key(&p.id))
        .ok()
        .flatten()
        .map(|z| z.to_string());

    let model = p.model.clone().unwrap_or_default();
    let ollama_num_ctx =
        (p.kind == "ollama").then(|| parse_ollama_extra(p.extra_json.as_deref()).num_ctx);

    let provider: Box<dyn Provider> = match p.kind.as_str() {
        "anthropic" => Box::new(AnthropicProvider::new(
            secret.ok_or_else(|| "missing API key for provider".to_string())?,
            p.base_url.clone(),
        )),
        "openai" => Box::new(OpenAiProvider::new(
            secret.ok_or_else(|| "missing API key for provider".to_string())?,
            p.base_url.clone(),
        )),
        // llama.cpp's server speaks the OpenAI wire format and needs no key.
        "llamacpp" => Box::new(OpenAiProvider::new(
            secret.unwrap_or_default(),
            p.base_url
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| Some("http://127.0.0.1:8080/v1".to_string())),
        )),
        "ollama" => Box::new(OllamaProvider::new(
            p.base_url.clone(),
            parse_ollama_extra(p.extra_json.as_deref()),
        )),
        "cursor" => Box::new(CliProvider::new(
            p.kind.clone(),
            p.bin_path
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| CliProvider::default_bin("cursor")),
            p.model.clone(),
            secret,
        )),
        "codex_cli" | "opencode_cli" => Box::new(CliProvider::new(
            p.kind.clone(),
            p.bin_path
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| CliProvider::default_bin(&p.kind)),
            p.model.clone(),
            None,
        )),
        other => return Err(format!("unknown provider kind: {other}")),
    };

    Ok(ResolvedProvider {
        provider,
        model,
        name: p.name,
        kind: p.kind,
        ollama_num_ctx,
    })
}

/// First enabled provider that can run the agent tool loop.
pub fn find_tool_provider_id(db: &Db) -> Option<String> {
    db.list_providers()
        .ok()?
        .into_iter()
        .find(|p| p.enabled && is_tool_capable_kind(&p.kind))
        .map(|p| p.id)
}

/// Resolve which provider should run this turn. Cursor uses xConsole MCP for
/// VPS SSH. OpenCode/Codex fall back to an API provider when one is configured.
pub fn resolve_for_turn(
    db: &Db,
    preferred_id: &str,
) -> Result<(ResolvedProvider, Option<String>), String> {
    let preferred = build(db, preferred_id)?;
    if !preferred.provider.is_autonomous_cli() {
        return Ok((preferred, None));
    }

    // Cursor Agent CLI runs SSH through xConsole MCP — keep the Cursor provider.
    if preferred.kind == "cursor" {
        return Ok((preferred, None));
    }

    let Some(fallback_id) = find_tool_provider_id(db) else {
        return Ok((preferred, None));
    };

    if fallback_id == preferred_id {
        return Ok((preferred, None));
    }

    let fallback = build(db, &fallback_id)?;
    let note = format!(
        "Active provider \"{}\" is chat-only; using \"{}\" to run tools on your servers.",
        preferred.name, fallback.name
    );
    Ok((fallback, Some(note)))
}

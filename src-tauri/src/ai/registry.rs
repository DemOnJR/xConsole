//! Build a live `Provider` from stored config + keychain secret. This is the one
//! place that maps a provider `kind` to an implementation.

use crate::ai::provider::Provider;
use crate::ai::providers::{
    anthropic::AnthropicProvider,
    cli::{CliProvider, RemoteTarget},
    ollama::{parse_ollama_extra, OllamaProvider},
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

/// Which provider and which model a turn should run on.
///
/// The two are separate choices. A persona that pins a provider is saying "use my
/// Anthropic account"; one that pins a model is saying "and use the cheap one on it".
/// Kept as a struct rather than two `Option<String>` parameters, which are trivially
/// swapped at a call site and fail silently when they are.
#[derive(Debug, Clone, Default)]
pub struct ModelChoice {
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

impl ModelChoice {
    /// The active provider and its configured model — what an ordinary desktop turn uses.
    pub fn active() -> Self {
        Self::default()
    }

    pub fn provider(provider_id: Option<String>) -> Self {
        Self { provider_id, model: None }
    }
}

/// Build a ready-to-use provider for the given provider id.
pub fn build(db: &Db, provider_id: &str) -> Result<ResolvedProvider, String> {
    build_with_model(db, provider_id, None)
}

/// Build a provider, optionally overriding the model it is configured with.
///
/// The override is applied to the stored row *before* the provider is constructed, not
/// to the returned struct: a CLI provider bakes the model into its `--model` flag at
/// construction, so patching `ResolvedProvider.model` afterwards would change what the
/// UI reports without changing what actually runs.
pub fn build_with_model(
    db: &Db,
    provider_id: &str,
    model_override: Option<&str>,
) -> Result<ResolvedProvider, String> {
    let mut p = db
        .get_provider(provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "provider not found".to_string())?;

    if let Some(m) = model_override.map(str::trim).filter(|m| !m.is_empty()) {
        p.model = Some(m.to_string());
    }

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
        "codex_cli" | "opencode_cli" | "antigravity_cli" | "claude_code" | "grok_cli" => {
            let remote = remote_target(db, &p);
            // Which machine's filesystem is worth probing depends on which machine it
            // will run on. `default_bin` looks under *this* desktop's home, and that
            // answer is worse than useless on a server.
            let bin = p.bin_path.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| {
                if remote.is_some() {
                    crate::ai::providers::cli::remote_default_bin(&p.kind).to_string()
                } else {
                    CliProvider::default_bin(&p.kind)
                }
            });
            Box::new(
                CliProvider::new(p.kind.clone(), bin, p.model.clone(), secret)
                    .with_remote(remote),
            )
        }
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

/// Where a CLI provider runs, read from its `extra_json`.
///
/// `{"vps_id": "...", "permission_mode": "acceptEdits"}`. Absent or blank `vps_id` means
/// this machine, which is the default and stays the default — moving an agent onto a
/// server is a decision the user makes explicitly, per provider.
fn remote_target(db: &Db, p: &crate::storage::models::AiProvider) -> Option<RemoteTarget> {
    if !crate::ai::providers::cli::is_cli_kind(&p.kind) {
        return None;
    }
    let extra: serde_json::Value = serde_json::from_str(p.extra_json.as_deref()?).ok()?;
    let vps_id = extra.get("vps_id")?.as_str()?.trim();
    if vps_id.is_empty() {
        return None;
    }
    Some(RemoteTarget {
        vps_id: vps_id.to_string(),
        db: db.clone(),
        permission_mode: extra
            .get("permission_mode")
            .and_then(|m| m.as_str())
            .unwrap_or("acceptEdits")
            .to_string(),
        // Absent means "log in as the VPS row says", which is what every existing row
        // does today. Never defaulted to a name: inventing an account that does not
        // exist turns every run into `sudo: unknown user`, and the account is created by
        // `agent_cli_provision`, not by reading a config field.
        run_as_user: extra
            .get("run_as_user")
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

#[cfg(test)]
mod remote_target_tests {
    use super::*;

    fn provider(kind: &str, extra: Option<&str>) -> crate::storage::models::AiProvider {
        crate::storage::models::AiProvider {
            id: "p1".into(),
            name: "test".into(),
            kind: kind.to_string(),
            model: None,
            base_url: None,
            bin_path: None,
            extra_json: extra.map(str::to_string),
            enabled: true,
            has_secret: false,
            created_at: None,
        }
    }

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn a_server_is_used_only_when_one_is_named() {
        let d = db();
        // Default is this machine. Anything else would move a user's agent onto a server
        // because a config field happened to exist.
        assert!(remote_target(&d, &provider("claude_code", None)).is_none());
        assert!(remote_target(&d, &provider("claude_code", Some("{}"))).is_none());
        assert!(remote_target(&d, &provider("claude_code", Some(r#"{"vps_id":"  "}"#))).is_none());
        assert!(remote_target(&d, &provider("claude_code", Some("not json"))).is_none());
    }

    #[test]
    fn any_agent_cli_can_be_pointed_at_a_server_but_an_api_provider_cannot() {
        let d = db();
        let cfg = Some(r#"{"vps_id":"web-1"}"#);
        assert!(remote_target(&d, &provider("claude_code", cfg)).is_some());
        assert!(remote_target(&d, &provider("grok_cli", cfg)).is_some());
        assert!(remote_target(&d, &provider("codex_cli", cfg)).is_some());
        // An HTTP API has no binary to run over SSH, so a vps_id on one is meaningless.
        assert!(remote_target(&d, &provider("anthropic", cfg)).is_none());
        assert!(remote_target(&d, &provider("openai", cfg)).is_none());
    }

    #[test]
    fn the_agent_user_is_read_from_the_row_and_never_invented() {
        let d = db();
        // Absent means "log in as the VPS row says" — the behaviour every existing
        // provider already has. A default name here would be an account that does not
        // exist, and every run would end in `sudo: unknown user`.
        let t = remote_target(&d, &provider("claude_code", Some(r#"{"vps_id":"web-1"}"#))).unwrap();
        assert_eq!(t.run_as_user, None);

        let t = remote_target(
            &d,
            &provider(
                "claude_code",
                Some(r#"{"vps_id":"web-1","run_as_user":" xconsole-agent "}"#),
            ),
        )
        .unwrap();
        assert_eq!(t.run_as_user.as_deref(), Some("xconsole-agent"));

        let t = remote_target(
            &d,
            &provider("claude_code", Some(r#"{"vps_id":"web-1","run_as_user":"   "}"#)),
        )
        .unwrap();
        assert_eq!(t.run_as_user, None);
    }

    #[test]
    fn the_permission_mode_carries_through_with_a_working_default() {
        let d = db();
        let t = remote_target(&d, &provider("claude_code", Some(r#"{"vps_id":"web-1"}"#))).unwrap();
        assert_eq!(t.vps_id, "web-1");
        // A remote target is configured on purpose for a named box, so it can act there.
        assert_eq!(t.permission_mode, "acceptEdits");

        let t = remote_target(
            &d,
            &provider("claude_code", Some(r#"{"vps_id":"web-1","permission_mode":"dontAsk"}"#)),
        )
        .unwrap();
        assert_eq!(t.permission_mode, "dontAsk");
    }
}

/// First enabled provider that can run the agent tool loop.
#[allow(dead_code)]
pub fn find_tool_provider_id(db: &Db) -> Option<String> {
    db.list_providers()
        .ok()?
        .into_iter()
        .find(|p| p.enabled && is_tool_capable_kind(&p.kind))
        .map(|p| p.id)
}

/// Resolve which provider should run this turn.
pub fn resolve_for_turn(
    db: &Db,
    preferred_id: &str,
    model_override: Option<&str>,
) -> Result<(ResolvedProvider, Option<String>), String> {
    let preferred = build_with_model(db, preferred_id, model_override)?;
    Ok((preferred, None))
}

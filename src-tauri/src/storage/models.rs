use serde::{Deserialize, Serialize};

/// Authentication method for a VPS connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    /// Authenticate through ssh-agent (safest: app never sees private key bytes).
    Agent,
    /// Private key file referenced by path (passphrase, if any, lives in the OS keychain).
    Key,
    /// Password (stored in the OS keychain, never in SQLite).
    Password,
}

impl AuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::Agent => "agent",
            AuthType::Key => "key",
            AuthType::Password => "password",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "agent" => AuthType::Agent,
            "password" => AuthType::Password,
            _ => AuthType::Key,
        }
    }
}

/// A saved VPS / host definition. No secrets are stored here: passwords and key
/// passphrases live in the OS keychain, private keys are referenced by path only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vps {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    /// Path to the private key file (for `AuthType::Key`). Never the key material itself.
    #[serde(default)]
    pub key_path: Option<String>,
    /// Free-form comma-separated tags for sidebar filtering.
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Payload to create or update a VPS (id optional on create).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
    /// Optional secret (password or key passphrase). Persisted to the OS keychain
    /// only; it is never written to SQLite and is dropped from memory after storing.
    #[serde(default)]
    pub secret: Option<String>,
}

/// A saved canvas workspace (named layout snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// Serialized canvas viewport (x, y, zoom) as JSON.
    #[serde(default)]
    pub viewport_json: Option<String>,
    /// Layout mode: "freeform" | "snap" | "tile".
    #[serde(default)]
    pub layout_mode: Option<String>,
    /// Serialized array of nodes (vps_id + position + size) as JSON.
    #[serde(default)]
    pub nodes_json: Option<String>,
    /// Accent color (hex) for the workspace.
    #[serde(default)]
    pub color: Option<String>,
    /// Icon (emoji) for the workspace.
    #[serde(default)]
    pub icon: Option<String>,
    /// Where the accent color is applied: "side" | "border" | "bg".
    #[serde(default)]
    pub color_mode: Option<String>,
    /// JSON describing the workspace's project location for agent context:
    /// `{ "kind": "local"|"vps", "path": "...", "vps_id": "..."? }`.
    #[serde(default)]
    pub project_json: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub viewport_json: Option<String>,
    #[serde(default)]
    pub layout_mode: Option<String>,
    #[serde(default)]
    pub nodes_json: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color_mode: Option<String>,
    #[serde(default)]
    pub project_json: Option<String>,
}

/// A configured AI provider. Secrets (API keys / tokens) are never stored here;
/// they live in the OS keychain under `ai:<id>:key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    /// "anthropic" | "openai" | "ollama" | "cursor" | "codex_cli" | "opencode_cli"
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Path to the CLI binary (for codex_cli / opencode_cli).
    #[serde(default)]
    pub bin_path: Option<String>,
    /// Free-form JSON for provider-specific options.
    #[serde(default)]
    pub extra_json: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether a secret is present in the keychain. Derived, not stored.
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProviderInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub bin_path: Option<String>,
    #[serde(default)]
    pub extra_json: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional secret (API key / token). Persisted only to the OS keychain.
    #[serde(default)]
    pub secret: Option<String>,
}

/// A scheduled agent job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    /// Cron-like schedule string (see `ai::cron`).
    pub schedule: String,
    /// "prompt" | "command"
    pub kind: String,
    /// The prompt text or the raw shell command.
    pub payload: String,
    /// JSON array of VPS ids to target.
    #[serde(default)]
    pub targets_json: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub schedule: String,
    pub kind: String,
    pub payload: String,
    #[serde(default)]
    pub targets_json: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// A pending or resolved approval for an agent command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentApproval {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub vps_id: Option<String>,
    pub command: String,
    /// "pending" | "approved" | "denied"
    pub status: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Saved agent chat thread (messages stored as JSON array).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConversation {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub targets_json: Option<String>,
    pub messages_json: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// List row for the conversation picker (no message payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConversationMeta {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConversationInput {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    /// JSON array of { role, content, activity? }
    pub messages_json: String,
}

fn default_true() -> bool {
    true
}

/// A Terraform project tracked locally and applied via a VPS runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraProject {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// blank | vps-web | aws-minimal | gcp-minimal
    pub template: String,
    /// vps | tfc — where state/runs live
    #[serde(default = "default_backend_vps")]
    pub backend: String,
    #[serde(default)]
    pub default_vps_id: Option<String>,
    #[serde(default)]
    pub cloud_account_id: Option<String>,
    /// JSON: tfc_org, tfc_workspace, aws_region, gcp_region, …
    #[serde(default)]
    pub config_json: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

fn default_backend_vps() -> String {
    "vps".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraProjectInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub default_vps_id: Option<String>,
    #[serde(default)]
    pub cloud_account_id: Option<String>,
    #[serde(default)]
    pub config_json: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Cloud provider connection. Credentials live in the OS keychain only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAccount {
    pub id: String,
    pub name: String,
    /// aws | gcp | tfc
    pub kind: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAccountInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    /// AWS keys, GCP SA JSON, or TFC token — keychain only.
    #[serde(default)]
    pub secret: Option<String>,
}

/// A pinned host key fingerprint (trust-on-first-use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    #[serde(default)]
    pub added_at: Option<String>,
}

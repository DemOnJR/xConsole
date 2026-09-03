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

/// Public-only login fields the agent may change. Never includes a password or key bytes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VpsLoginPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub auth_type: Option<AuthType>,
    #[serde(default)]
    pub key_path: Option<String>,
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
    /// "anthropic" | "openai" | "ollama" | "cursor" | "codex_cli" | "opencode_cli" | "antigravity_cli"
    pub kind: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Path to the CLI binary (for codex_cli / opencode_cli / antigravity_cli).
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
    /// Project this job is about. The run gets that project's brief and files its work
    /// there — a scheduled review of "the project" is meaningless without one.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// The named agent the job runs as. `None` runs it as the main agent.
    ///
    /// This is what makes a schedule a member of staff rather than a script: the review
    /// happens as the lead of that project, under its instructions and its trust level,
    /// and what it says goes into that team's thread.
    #[serde(default)]
    pub persona_id: Option<String>,
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
    /// Project this job is about.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// The named agent it runs as. `None` = the main agent.
    #[serde(default)]
    pub persona_id: Option<String>,
}

/// A persistent goal session (the /goal autonomous mode). The loop controller in
/// `ai::goal` drives plan → act → verify cycles against a GoalSpec; kanban cards
/// and constraint memory live as JSON blobs so the schema stays stable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSession {
    pub id: String,
    /// Short label for the kanban node header.
    pub title: String,
    /// The user's original "/goal ..." text.
    pub raw_request: String,
    /// Serialized GoalSpec.
    pub spec_json: String,
    /// "intake" | "active" | "paused" | "waiting" | "blocked" | "done" | "stopped"
    pub status: String,
    /// Serialized Vec<GoalTask>.
    pub kanban_json: String,
    /// Serialized constraint memory (facts learned during the run).
    pub memory_json: String,
    /// RFC3339; when "waiting", when to resume.
    #[serde(default)]
    pub next_check_at: Option<String>,
    /// How many plan→act→verify cycles have run.
    pub cycles: i64,
    /// The persona running this goal. None = the default agent.
    #[serde(default)]
    pub persona_id: Option<String>,
    /// The project this task belongs to, so a delegated agent knows what it is working
    /// on and the user can see one project's work without the others in the way.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// What actually came of it, in the agent's own words.
    ///
    /// The evidence it cites when it declares the goal met, or why it stopped when it
    /// did not. Without this a finished task is a title and a status — enough to say
    /// something happened, not enough to say whether it worked, which is the only
    /// question worth asking a week later.
    #[serde(default)]
    pub outcome: Option<String>,
    /// The chat that asked for this work. Without it, finishing has nobody to tell.
    #[serde(default)]
    pub request_id: Option<String>,
    /// When this task's result was passed up the chain, so it is passed up once.
    #[serde(default)]
    pub reported_at: Option<String>,
    /// The pull request this task opened, if any.
    #[serde(default)]
    pub pr_number: Option<i64>,
    /// Where a new-feature proposal has got to: NULL for ordinary improvement work,
    /// otherwise proposed | ceo_ok | orch_ok | approved | rejected.
    #[serde(default)]
    pub approval_state: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// A named agent the user can hand work to.
///
/// The user asked for agents with names and roles that go away, do the job, and come
/// back when they are done. That behaviour already exists in the goal loop; what was
/// missing was an identity to run it under. A persona supplies one: its own standing
/// instructions, the servers it works on, how much it is trusted, and optionally its
/// own model — so a persona doing routine log triage need not run on the same
/// expensive model as one making architecture calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    /// What the user calls it: "Ada", "CEO", "night-shift".
    pub name: String,
    /// One line on what it is for. Shown in pickers and injected into its prompt.
    #[serde(default)]
    pub role: String,
    /// Standing instructions, appended to the agent's soul for this persona's runs.
    #[serde(default)]
    pub instructions: String,
    /// VPS ids this persona works on unless a task says otherwise.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Overrides the global safety mode when set.
    #[serde(default)]
    pub safety_mode: Option<String>,
    /// Overrides the active provider when set.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Overrides the provider's model when set.
    #[serde(default)]
    pub model: Option<String>,
    pub enabled: bool,
    /// The persona this one reports to. None = reports to the user directly.
    #[serde(default)]
    pub reports_to: Option<String>,
    /// The project this agent works on. `None` makes it company-wide.
    ///
    /// One team per project is the whole shape of this: with several projects running,
    /// "the reviewer" is ambiguous until you say whose reviewer, routing has nothing to
    /// route on, and a per-team record of what was done cannot be assembled. The
    /// exception is the handful of agents that answer across everything — the one the
    /// user actually talks to, above all — which stay unassigned deliberately.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Globs this persona may write under, relative to the project root. Empty = the
    /// whole project root, which is what every persona created before scoping had.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Tool names this persona may call. Empty = every tool. A reviewer that cannot
    /// write and an engineer that cannot touch DNS are the same mechanism.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// One message between agents (or to the user).
///
/// The user asked to be able to watch what the agents say to each other, so the
/// exchange is stored as its own rows rather than left inside each agent's private
/// transcript — otherwise "what did the programmer tell the CEO" is unanswerable
/// without reading two separate conversations and guessing at the join.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    /// Sender persona id. None = the user.
    #[serde(default)]
    pub from_id: Option<String>,
    /// Recipient persona id. None = the user.
    #[serde(default)]
    pub to_id: Option<String>,
    /// "report" (upward), "request" (downward or sideways), "note".
    pub kind: String,
    pub body: String,
    /// The delegated task this concerns, when there is one.
    #[serde(default)]
    pub goal_id: Option<String>,
    /// The project this was said about.
    ///
    /// Without it every agent's messages land in one pool, and "what did the reviewer
    /// say" returns three answers from three unrelated codebases with no way to tell
    /// which is which. None means genuinely un-scoped: messages predating projects, or
    /// an exchange that belongs to no single one.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Set when the message was delivered into the recipient's prompt.
    ///
    /// Delivery is not handling: a bug report is shown once and then looks dealt with
    /// whether or not anything was fixed. See `resolved_at`.
    #[serde(default)]
    pub read_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    /// The room this was said in. `None` on every row written before channels existed;
    /// those are routed by the client's original derivation so history does not vanish.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// The message or log line this replies to, making it a thread.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Persona ids named in the body, so a room post can reach someone specific.
    #[serde(default)]
    pub mentions: Vec<String>,
    /// Set only by an explicit, evidenced resolution -- not by delivery.
    #[serde(default)]
    pub resolved_at: Option<String>,
}

/// Fields accepted when creating or updating a persona.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub safety_mode: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The persona this one reports to. None = reports to the user directly.
    #[serde(default)]
    pub reports_to: Option<String>,
    /// The project this agent works on. `None` makes it company-wide.
    ///
    /// One team per project is the whole shape of this: with several projects running,
    /// "the reviewer" is ambiguous until you say whose reviewer, routing has nothing to
    /// route on, and a per-team record of what was done cannot be assembled. The
    /// exception is the handful of agents that answer across everything — the one the
    /// user actually talks to, above all — which stay unassigned deliberately.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Globs this persona may write under, relative to the project root. Empty = the
    /// whole project root, which is what every persona created before scoping had.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Tool names this persona may call. Empty = every tool. A reviewer that cannot
    /// write and an engineer that cannot touch DNS are the same mechanism.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// The locked-in definition of "done" for a goal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalSpec {
    pub objective: String,
    pub success_criteria: Vec<String>,
    /// How the agent verifies progress (tool names + procedure).
    pub check_method: String,
    /// Tool names allowed for verification, e.g. ["web_search", "run_command"].
    #[serde(default)]
    pub check_tooling: Vec<String>,
    /// Things the agent must never do.
    #[serde(default)]
    pub hard_constraints: Vec<String>,
    /// Safety valve; None = unbounded but still stoppable.
    #[serde(default)]
    pub max_cycles: Option<i64>,
    /// VPS ids the loop may act on (copied from the agent picker at lock time).
    #[serde(default)]
    pub vps_targets: Vec<String>,
}

/// One event on a kanban card (created / moved / result / note / …).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalTaskEvent {
    pub at: String,
    pub action: String,
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One kanban card for a goal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalTask {
    pub id: String,
    /// "backlog" | "in_progress" | "waiting" | "testing" | "blocked" | "done"
    pub column: String,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    /// "edit" | "test" | "bug" | "research" | "check"
    #[serde(default)]
    pub kind: String,
    /// Touched file paths.
    #[serde(default)]
    pub files: Vec<String>,
    /// Outcome once resolved.
    #[serde(default)]
    pub result: Option<String>,
    /// Populated when kind="bug" or a test failed.
    #[serde(default)]
    pub error: Option<String>,
    /// Parent card id when this is a sub-task. Roots have `None`.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// What happened on this card, oldest first.
    #[serde(default)]
    pub history: Vec<GoalTaskEvent>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
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

/// A plan the agent presented via `present_plan` (persisted for history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub plan: String,
    /// presented | applied | archived | cancelled
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// List row for the plan history picker (no plan body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanMeta {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
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

/// A remembered database login. The password lives in the OS keychain, never here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConnection {
    pub id: String,
    pub vps_id: String,
    /// Discovery's endpoint id, so this can be matched to an instance after a rescan.
    pub endpoint_id: String,
    /// "mysql" | "postgres" | "redis".
    pub engine: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub container: Option<String>,
    pub username: String,
    #[serde(default)]
    pub database: Option<String>,
    /// Whether a password is actually present in the keychain for this row.
    #[serde(default)]
    pub has_secret: bool,
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

/// Audit log entry for Cloudflare edits with before/after state for instant rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareAuditLog {
    pub id: String,
    pub account_id: String,
    pub action_type: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_name: Option<String>,
    pub summary: String,
    pub actor: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub before_state: Option<String>,
    #[serde(default)]
    pub after_state: Option<String>,
    pub reverted: bool,
    pub created_at: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareAuditLogInput {
    pub account_id: String,
    pub action_type: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_name: Option<String>,
    pub summary: String,
    pub actor: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub before_state: Option<String>,
    #[serde(default)]
    pub after_state: Option<String>,
}

/// One thing an agent actually did, kept durably.
///
/// The live `agent://persona-status` event is fire-and-forget with a client-side TTL, so
/// a per-agent log channel built on it alone is empty after a restart and invisible to
/// every other agent. These rows are what makes "what has Ada been doing" answerable by
/// a teammate rather than only by whoever had the window open at the time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogEntry {
    pub id: String,
    pub persona_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    /// The agent run this line came from. Empty when it came from outside a run.
    #[serde(default)]
    pub session_id: String,
    /// The phase word the status feed uses: working, thinking, verifying, blocked...
    pub status: String,
    /// The tool that produced it, when one did.
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// How much one reader has not yet seen in one room.
///
/// `last_read_at` travels with the counts deliberately: the client recomputes against
/// the cursor as live messages arrive, so a count taken at load does not go stale the
/// moment somebody says something.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelUnread {
    pub channel_id: String,
    /// Messages in the room this reader has not seen. Their own never count.
    pub unread: i64,
    /// How many of those name this reader. Always 0 for the user, who is not a persona.
    pub mentions: i64,
    /// Where this reader's cursor sits, or None if they have never opened the room.
    #[serde(default)]
    pub last_read_at: Option<String>,
}

/// A new feature an agent wants to build, and how far up the chain it has got.
///
/// Improving what exists is what an agent is for and needs nobody's permission.
/// Building something *new* is a decision about what the product is, and that belongs
/// to whoever is accountable for it. The difference lives here, in a row, rather than
/// in a sentence in a prompt that a model may or may not honour: while a proposal is
/// undecided its goal is marked `proposed`, and the tool dispatcher will not let that
/// goal write anything but documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureProposal {
    pub id: String,
    /// The project it is for. None for a company-wide proposal.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Who proposed it. None when the main agent did.
    #[serde(default)]
    pub persona_id: Option<String>,
    /// The task it came out of, if it came out of one.
    #[serde(default)]
    pub goal_id: Option<String>,
    pub title: String,
    pub body: String,
    /// proposed | at_ceo | at_orchestrator | approved | rejected
    pub state: String,
    /// Persona id, or None when the user decided it themselves.
    #[serde(default)]
    pub decided_by: Option<String>,
    #[serde(default)]
    pub decision_note: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

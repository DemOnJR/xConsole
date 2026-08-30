import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PluginManifest } from "../sdk/plugin";

export type AuthType = "agent" | "key" | "password";

export interface Vps {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: AuthType;
  key_path?: string | null;
  tags?: string | null;
  created_at?: string | null;
}

/** A file the agent created on this PC (SSH key backup, download, write). */
export interface Artifact {
  id: string;
  name: string;
  path: string;
  kind: string;
  sha256: string;
  size: number;
  secret: boolean;
  session_id?: string | null;
  vps_id?: string | null;
  created_at?: string | null;
}

export interface VpsInput {
  id?: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: AuthType;
  key_path?: string | null;
  tags?: string | null;
  /** Password or key passphrase; stored only in the OS keychain. */
  secret?: string | null;
}

export type ColorMode = "side" | "border" | "bg";

export interface Workspace {
  id: string;
  name: string;
  viewport_json?: string | null;
  layout_mode?: string | null;
  nodes_json?: string | null;
  color?: string | null;
  icon?: string | null;
  color_mode?: string | null;
  /** JSON: { kind: "local"|"vps", path, vps_id? } — the workspace's project location. */
  project_json?: string | null;
  updated_at?: string | null;
}

export interface WorkspaceInput {
  id?: string;
  name: string;
  viewport_json?: string | null;
  layout_mode?: string | null;
  nodes_json?: string | null;
  color?: string | null;
  icon?: string | null;
  color_mode?: string | null;
  project_json?: string | null;
}

/** A workspace's project location, used for agent context. */
export interface WorkspaceProject {
  kind: "local" | "vps";
  path: string;
  vps_id?: string;
}

/** RAM/GPU snapshot for model-fit filtering. */
export interface SystemCaps {
  ram_mb: number;
  vram_mb: number | null;
  gpu_name: string | null;
}

export interface ResourceSnapshot {
  ts: string;
  cpu_pct: number;
  ram_mb: number;
  ram_total_mb: number;
  process_ram_mb: number;
  gpu_pct: number | null;
  gpu_mem_mb: number | null;
  gpu_mem_total_mb: number | null;
  gpu_name: string | null;
}

export interface AgentAnalytics {
  cache: { ts: string; session: string; prompt: number; hit: number; miss: number; pct: number }[];
  cache_avg_pct: number;
  conversations: {
    id: string;
    title: string;
    updated_at: string;
    user_turns: number;
    tool_calls: number;
    tools: { name: string; count: number }[];
  }[];
  tools_all: { name: string; count: number }[];
  resource: ResourceSnapshot;
}

export interface ModelEntry {
  id: string;
  name: string;
  source: "ollama" | "huggingface";
  size_bytes: number | null;
  detail: string;
  installed: boolean;
}

export interface HfFile {
  file: string;
  size_bytes: number;
  url: string;
}

export interface LocalFile {
  file: string;
  size_bytes: number;
}

export interface LlamaStatus {
  running: boolean;
  port: number | null;
  model: string | null;
  bin: string | null;
}

export interface OllamaStatus {
  installed: boolean;
  running: boolean;
  bin: string | null;
}

export interface DownloadProgress {
  id: string;
  received: number;
  total: number | null;
  status: "downloading" | "done" | "error";
  message: string | null;
}

/** Result of a skill security scan (NVIDIA SkillSpector or built-in heuristic). */
export interface SkillScanReport {
  risk_score: number;
  severity: string;
  recommendation: string;
  findings: string[];
  scanner: string;
}

export interface ScannerStatus {
  installed: boolean;
  version: string | null;
  engine: string;
  uv_available: boolean;
}

/** Git work-tree status for a path (terminal cwd / SFTP browse path). */
export interface GitInfo {
  branch: string;
  dirty: boolean;
  root?: string | null;
}

export interface ConnectOutcome {
  session_id: string;
  vps_id: string;
  /** "match" | "pinned_on_first_use" | "mismatch" */
  host_key: string;
}

export interface SftpConnectOutcome {
  session_id: string;
  vps_id: string;
  path: string;
}

export interface SftpEntry {
  name: string;
  path: string;
  /** True if this opens as a directory — already resolved through any symlink. */
  is_dir: boolean;
  size: number;
  /** The entry itself is a symbolic link, whatever it points at. */
  is_symlink: boolean;
  /** The target as stored, relative if it was written relative. Null unless a link. */
  link_target: string | null;
  /** A link whose target does not resolve. */
  link_broken: boolean;
}

/** Local filesystem listing for dual-pane SFTP. */
export interface LocalFsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

export interface LocalFsList {
  path: string;
  entries: LocalFsEntry[];
}

// ----- Database client -----

/** An engine the client can actually drive. */
export type DbEngine = "mysql" | "postgres" | "redis";

/** Any database product discovery can recognise, browsable or not. */
export type DbProduct =
  | "mysql"
  | "postgres"
  | "mongodb"
  | "redis"
  | "mssql"
  | "clickhouse"
  | "cassandra"
  | "couchdb"
  | "elasticsearch";

/** Human label for a product, for the tree. */
export const DB_PRODUCT_LABEL: Record<DbProduct, string> = {
  mysql: "MySQL / MariaDB",
  postgres: "PostgreSQL",
  mongodb: "MongoDB",
  redis: "Redis / Valkey",
  mssql: "SQL Server",
  clickhouse: "ClickHouse",
  cassandra: "Cassandra",
  couchdb: "CouchDB",
  elasticsearch: "Elasticsearch",
};

/** A database server found on a host (matches the Rust `DbEndpoint`). */
export interface DbEndpoint {
  id: string;
  label: string;
  kind: "native" | "docker";
  host: string;
  port: number;
  container: string | null;
  image: string | null;
  /** What was found. */
  product: DbProduct;
  /** How to browse it — `null` when the client can't open this product yet. */
  engine: DbEngine | null;
}

/** Credentials + location for a connection (matches the Rust `DbTarget`). */
export interface DbTargetInput {
  vps_id: string;
  container: string | null;
  host: string;
  port: number;
  user: string;
  password: string;
  database: string | null;
  engine: DbEngine;
}

export interface DbConnectOutcome {
  session_id: string;
  version: string;
}

/** A remembered database login (matches the Rust `DbConnection`). The password is not
 *  here — it's in the OS keychain and never travels to the webview. */
export interface DbSavedConnection {
  id: string;
  vps_id: string;
  endpoint_id: string;
  engine: DbEngine;
  host: string;
  port: number;
  container: string | null;
  username: string;
  database: string | null;
  has_secret: boolean;
}

export interface DbTable {
  name: string;
  kind: string;
  rows: number;
  bytes: number;
  engine: string;
}

export interface DbColumn {
  name: string;
  data_type: string;
  nullable: boolean;
  primary: boolean;
  default: string;
  extra: string;
}

/** A tabular result. `null` in a cell is SQL NULL. */
export interface DbResultSet {
  columns: string[];
  rows: (string | null)[][];
  affected: number | null;
  message: string | null;
}

/** Primary-key identification of a row, as `[column, value]` pairs. */
export type DbRowKey = [string, string | null][];

export type TransferDirection = "download" | "upload";
export type ArchiveFormat = "targz" | "zip";
export type TransferFileState = "pending" | "active" | "done" | "failed" | "skipped";
export type TransferJobState =
  | "scanning"
  | "running"
  | "done"
  | "failed"
  | "cancelled";

/** One file inside a transfer job (matches the Rust `FileProgress`). */
export interface TransferFile {
  name: string;
  remote_path: string;
  local_path: string;
  size: number;
  transferred: number;
  state: TransferFileState;
  error: string | null;
}

/** A whole-job progress snapshot (matches the Rust `TransferSnapshot`). */
export interface TransferSnapshot {
  id: string;
  direction: TransferDirection;
  state: TransferJobState;
  label: string;
  files_total: number;
  files_done: number;
  bytes_total: number;
  bytes_done: number;
  elapsed_ms: number;
  eta_ms: number | null;
  bytes_per_sec: number;
  files: TransferFile[];
  error: string | null;
  destination: string | null;
}

export interface SftpListOutcome {
  path: string;
  entries: SftpEntry[];
}

export interface RemoteFileStat {
  mode: string;
  owner: string;
  group: string;
  is_dir: boolean;
}

export interface KnownHost {
  host: string;
  port: number;
  key_type: string;
  fingerprint: string;
  added_at?: string | null;
}

export type ProviderKind =
  | "anthropic"
  | "openai"
  | "ollama"
  | "llamacpp"
  | "cursor"
  | "codex_cli"
  | "opencode_cli"
  | "antigravity_cli"
  | "claude_code";



/** Which chat platform a remote-control bridge speaks. */
export type RemoteKind = "discord" | "telegram" | "whatsapp";

/** One transport's configuration. Bot tokens are never returned. */
export interface TransportStatus {
  kind: RemoteKind;
  enabled: boolean;
  chat_id: string;
  allowed_user_ids: string;
  has_token: boolean;
  /** Whether this platform needs a credential pasted in at all. WhatsApp does not. */
  needs_token: boolean;
  /** Whether this platform refuses to arm without a chat id. Only Discord does. */
  chat_required: boolean;
  /** False when this transport would refuse to run, so the UI can say why. */
  usable: boolean;
}

/** Remote-control status: the shared settings plus every transport. */
export interface RemoteStatus {
  /** The master switch. Every transport is off while this is. */
  enabled: boolean;
  prefix: string;
  safety_mode: string;
  targets: string[];
  /** The named agent that answers, if one is set. */
  persona_id: string | null;
  /** Its name, so the UI can say who is answering without a second lookup. */
  persona_name: string | null;
  transports: TransportStatus[];
  /** True when at least one transport is armed. */
  usable: boolean;
  /** Transport the shared conversation is on — where the user last spoke. */
  last_route: string | null;
  /** Messages in the shared thread. */
  conversation_len: number;
}

/** One chat the WhatsApp bridge can be restricted to. */
export interface WhatsAppChat {
  /** Full JID — what the bridge matches an incoming chat against. */
  id: string;
  name: string;
  /** "self" (Note to Self) or "group". */
  kind: string;
}

/** Pairing state for the WhatsApp bridge. */
export interface WhatsAppStatus {
  /** The sidecar binary was found. False means WhatsApp needs building or is not ready yet. */
  available: boolean;
  /** Currently building/installing the helper binary or Go toolchain. */
  building?: boolean;
  /** Progress step message (e.g. "Downloading Go compiler...", "Building WhatsApp helper..."). */
  build_step?: string | null;
  running: boolean;
  connected: boolean;
  /** A device is paired. Survives restarts — the session lives on disk. */
  linked: boolean;
  jid?: string | null;
  phone?: string | null;
  push_name?: string | null;
  /** The pairing QR, already rendered as SVG by the Rust side. */
  qr_svg?: string | null;
  error?: string | null;
}

/** A named background agent: an identity the autonomous goal loop runs under. */
export interface Persona {
  id: string;
  name: string;
  role: string;
  instructions: string;
  targets: string[];
  safety_mode?: string | null;
  provider_id?: string | null;
  model?: string | null;
  enabled: boolean;
  /** Who this agent reports to. null = reports to you directly. */
  reports_to?: string | null;
  /** The project this agent works on. Null = company-wide, answers on any project. */
  workspace_id?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

export interface PersonaInput {
  id?: string;
  name: string;
  role: string;
  instructions: string;
  targets: string[];
  safety_mode?: string | null;
  provider_id?: string | null;
  model?: string | null;
  enabled: boolean;
  reports_to?: string | null;
  /** The project this agent works on. Null = company-wide, answers on any project. */
  workspace_id?: string | null;
}

/** One message between agents. `from_id`/`to_id` null means the user. */
export interface AgentMessage {
  id: string;
  from_id?: string | null;
  to_id?: string | null;
  kind: string;
  body: string;
  goal_id?: string | null;
  /** The project this was said about. Null for anything genuinely cross-project. */
  workspace_id?: string | null;
  read_at?: string | null;
  created_at?: string | null;
}

/** One agent's record over a window. */
export interface AgentActivity {
  persona_id: string;
  name: string;
  /** The project it belongs to, if any. */
  project?: string | null;
  days: number;
  tasks: GoalSession[];
  changes: FileChange[];
  messages: AgentMessage[];
}

/** How one of a project's numbers moved against the period before. */
export interface MetricMovement {
  name: string;
  current: number;
  previous: number;
  /** Null when the previous period was zero — a first sale is not a percentage. */
  change_pct?: number | null;
  unit?: string | null;
  days_with_data: number;
}

/** One commit in a project's repository. */
export interface Commit {
  sha: string;
  author: string;
  date: string;
  subject: string;
}

/**
 * Everything one project has to show for itself.
 *
 * Four sources assembled per project — tasks, conversation, file changes, commits —
 * rather than four screens the user has to correlate by timestamp.
 */
export interface ProjectHistory {
  workspace_id: string;
  name: string;
  location?: string | null;
  tasks: GoalSession[];
  messages: AgentMessage[];
  changes: FileChange[];
  branch?: string | null;
  commits: Commit[];
  /** Why the git half is empty, when it is. */
  git_note?: string | null;
}

export interface AiProvider {
  id: string;
  name: string;
  kind: ProviderKind;
  model?: string | null;
  base_url?: string | null;
  bin_path?: string | null;
  extra_json?: string | null;
  enabled: boolean;
  has_secret: boolean;
  created_at?: string | null;
}

export interface AiProviderInput {
  id?: string;
  name: string;
  kind: ProviderKind;
  model?: string | null;
  base_url?: string | null;
  bin_path?: string | null;
  extra_json?: string | null;
  enabled: boolean;
  /** API key / token; stored only in the OS keychain. */
  secret?: string | null;
}

export interface Setting {
  key: string;
  value: string;
}

export interface AgentApproval {
  id: string;
  session_id: string;
  vps_id?: string | null;
  command: string;
  status: string;
  created_at?: string | null;
}

/** A clarifying question the agent asks via the ask_user tool. */
export interface AgentQuestionItem {
  question: string;
  header?: string;
  options?: string[];
  multi?: boolean;
}

export interface AgentQuestion {
  id: string;
  session_id: string;
  questions: AgentQuestionItem[];
}

/** A plan the agent presents via present_plan, awaiting approve/reject. */
export interface AgentPlan {
  id: string;
  session_id: string;
  workspace_id?: string | null;
  title?: string;
  plan: string;
}

/** A plan record from the persisted plan history (no body). */
export interface AgentPlanMeta {
  id: string;
  session_id: string;
  workspace_id?: string | null;
  title?: string | null;
  status: "presented" | "applied" | "archived" | "cancelled";
  created_at?: string | null;
  updated_at?: string | null;
}

/** A single persisted plan with its full body. */
export interface AgentPlanFull extends AgentPlanMeta {
  plan: string;
}

export interface AgentConversationMeta {
  id: string;
  title: string;
  summary?: string | null;
  updated_at?: string | null;
}

export interface AgentConversation extends AgentConversationMeta {
  targets_json?: string | null;
  messages_json: string;
  created_at?: string | null;
}

export interface AgentDocs {
  soul: string;
  memory: string;
  taste: string;
}

/** Per-event hook counts + enable state for the Hooks settings section. */
export interface HooksStatus {
  enabled: boolean;
  total: number;
  pre_tool_use: number;
  post_tool_use: number;
  user_prompt_submit: number;
  stop: number;
  error: string | null;
}

export interface Skill {
  category: string;
  name: string;
  description: string;
}

export interface CronJob {
  id: string;
  name: string;
  schedule: string;
  kind: string;
  payload: string;
  targets_json?: string | null;
  enabled: boolean;
  last_run?: string | null;
  last_status?: string | null;
  /** Project this job is about. The run gets that project's brief. */
  workspace_id?: string | null;
  /** The named agent it runs as. Null = the main agent. */
  persona_id?: string | null;
  created_at?: string | null;
}

export interface CronJobInput {
  id?: string;
  name: string;
  schedule: string;
  kind: string;
  payload: string;
  targets_json?: string | null;
  enabled: boolean;
  /** Project this job is about. The run gets that project's brief and files work there. */
  workspace_id?: string | null;
  /** The named agent it runs as. Null = the main agent. */
  persona_id?: string | null;
}

/** A persistent autonomous goal session (/goal). */
export interface GoalSession {
  id: string;
  title: string;
  raw_request: string;
  spec_json: string;
  status: "intake" | "active" | "paused" | "waiting" | "blocked" | "done" | "stopped" | string;
  kanban_json: string;
  memory_json: string;
  next_check_at?: string | null;
  cycles: number;
  created_at?: string | null;
  updated_at?: string | null;
  finished_at?: string | null;
  /** The named agent running this task. Null = the default agent. */
  persona_id?: string | null;
  /** The project this task belongs to. */
  workspace_id?: string | null;
  /** What came of it, in the agent's own words. Null while it is still running. */
  outcome?: string | null;
}

/** The locked-in definition of "done" for a goal. */
export interface GoalSpec {
  objective: string;
  success_criteria: string[];
  check_method: string;
  check_tooling?: string[];
  hard_constraints?: string[];
  max_cycles?: number | null;
  vps_targets?: string[];
}

/** One event on a kanban card (created / moved / result / note / …). */
export interface GoalTaskEvent {
  at: string;
  action: string;
  column?: string | null;
  note?: string | null;
}

/** One kanban card. Sub-tasks are other cards whose `parent_id` is this id. */
export interface GoalTask {
  id: string;
  column: string;
  title: string;
  detail?: string | null;
  kind?: string;
  files?: string[];
  result?: string | null;
  error?: string | null;
  parent_id?: string | null;
  history?: GoalTaskEvent[];
  created_at?: string | null;
  updated_at?: string | null;
}

/** Constraint memory: learned facts + history. */
export interface GoalMemory {
  learned: { key: string; value: string; evidence: string; confidence: string }[];
  history?: { at: string; action: string; next_check: string }[];
}

export interface InfraProject {
  id: string;
  name: string;
  slug: string;
  template: string;
  backend: string;
  default_vps_id?: string | null;
  cloud_account_id?: string | null;
  config_json?: string | null;
  description?: string | null;
  created_at?: string | null;
}

export interface InfraProjectInput {
  id?: string;
  name: string;
  slug?: string | null;
  template?: string | null;
  backend?: string | null;
  default_vps_id?: string | null;
  cloud_account_id?: string | null;
  config_json?: string | null;
  description?: string | null;
}

export type CloudKind = "aws" | "gcp" | "tfc" | "cloudflare";

export interface CloudAccount {
  id: string;
  name: string;
  kind: CloudKind | string;
  region?: string | null;
  project_id?: string | null;
  organization?: string | null;
  has_secret: boolean;
  created_at?: string | null;
}

export interface CloudAccountInput {
  id?: string;
  name: string;
  kind: CloudKind | string;
  region?: string | null;
  project_id?: string | null;
  organization?: string | null;
  secret?: string | null;
}

export interface CloudflareZone {
  id: string;
  name: string;
  status: string;
  paused: boolean;
  type?: string | null;
}

export interface CloudflareTunnelConnection {
  id?: string | null;
  colo_name?: string | null;
  is_pending_reconnect?: boolean | null;
  opened_at?: string | null;
}

export interface CloudflareTunnel {
  id: string;
  name: string;
  status?: string | null;
  created_at?: string | null;
  conns_active_at?: string | null;
  connections: CloudflareTunnelConnection[];
}

export interface CloudflareIngressRule {
  hostname?: string | null;
  path?: string | null;
  service: string;
}

export interface CloudflareTunnelConfig {
  ingress: CloudflareIngressRule[];
}

export interface CloudflareDnsRecord {
  id: string;
  zone_id: string;
  zone_name: string;
  name: string;
  type: string;
  content: string;
  proxiable: boolean;
  proxied: boolean;
  ttl: number;
  comment?: string | null;
  created_on?: string | null;
  modified_on?: string | null;
}

export interface CloudflareDnsRecordInput {
  id?: string | null;
  name: string;
  type: string;
  content: string;
  proxied: boolean;
  ttl: number;
  comment?: string | null;
}

export interface CloudflareSecuritySettings {
  security_level: string;
  ssl?: string | null;
  attack_mode: boolean;
}

export interface CloudflareAuditLog {
  id: string;
  account_id: string;
  action_type: string;
  target_id?: string | null;
  target_name?: string | null;
  summary: string;
  actor: string;
  session_id?: string | null;
  before_state?: string | null;
  after_state?: string | null;
  reverted: boolean;
  created_at: string;
  ts: number;
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

export type ChatRole = "user" | "assistant" | "tool" | "system";

export interface ChatImage {
  media_type: string;
  data: string;
  name?: string;
}

export interface ChatMessage {
  role: ChatRole;
  content: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string | null;
  activity?: AgentActivityItem[];
  images?: ChatImage[];
}

export interface DiffLine {
  kind: "add" | "del" | "ctx" | string;
  text: string;
}

export interface AgentActivityItem {
  id: string;
  kind: "status" | "tool" | "skill_read" | "skill_save" | "command" | "tool_end" | "file_edit";
  label: string;
  detail?: string;
  output?: string;
  state: "running" | "done" | "error";
  category?: string;
  name?: string;
  tool?: string;
  path?: string;
  linesAdded?: number;
  linesRemoved?: number;
  hunks?: DiffLine[];
}

/** Mirrors the Rust `ActivityEvent` enum (serde tag="type", content="data"). */
export type ActivityEvent =
  | { type: "ToolStart"; data: { id: string; tool: string; label: string; detail?: string } }
  | { type: "ToolEnd"; data: { id: string; ok: boolean } }
  | { type: "SkillRead"; data: { id: string; category: string; name: string } }
  | { type: "SkillSaved"; data: { id: string; category: string; name: string } }
  | { type: "Command"; data: { id: string; vps: string; command: string } }
  | {
      type: "FileEdit";
      data: {
        id: string;
        path: string;
        lines_added: number;
        lines_removed: number;
        hunks: DiffLine[];
      };
    };

/** Mirrors the Rust `StreamEvent` enum (serde tag="kind", content="data"). */
export type StreamEvent =
  | { kind: "Text"; data: string }
  | { kind: "Status"; data: string }
  | { kind: "ToolCall"; data: ToolCall }
  | { kind: "ToolResult"; data: { id: string; output: string } }
  | { kind: "Activity"; data: ActivityEvent }
  | {
      kind: "Stats";
      data: {
        completion_tokens: number;
        prompt_tokens?: number | null;
        cached_tokens?: number | null;
        cache_creation_tokens?: number | null;
        duration_ms: number;
        tokens_per_sec: number;
      };
    }
  | {
      kind: "Cost";
      data: {
        input_tokens: number;
        output_tokens: number;
        cache_read_tokens: number;
        cache_write_tokens: number;
        usd: number;
      };
    }
  | {
      kind: "TurnTelemetry";
      data: {
        tool_calls: number;
        tool_cache_lookups: number;
        tool_cache_hits: number;
        tool_cache_misses: number;
        tool_cache_writes: number;
        tool_cache_hit_rate: number;
      };
    }
  | {
      kind: "PrefixTelemetry";
      data: {
        request_index: number;
        system_hash: string;
        schema_hash: string;
        message_prefix_hash: string;
        system_bytes: number;
        schema_bytes: number;
        message_bytes: number;
        classification: string;
        source: string;
      };
    }
  | {
      kind: "ContextUsage";
      data: {
        segments: { key: string; label: string; tokens: number }[];
        total_tokens: number;
        context_limit: number;
        percent: number;
      };
    }
  | { kind: "ConversationCompacted"; data: { messages: ChatMessage[] } }
  | { kind: "Done" }
  | { kind: "Error"; data: string };

export type SessionStatus =
  | { kind: "Connecting" }
  | { kind: "Connected" }
  | { kind: "Reconnecting" }
  | { kind: "Disconnected" }
  | { kind: "Error"; detail: string };

export const api = {
  listVps: () => invoke<Vps[]>("list_vps"),
  saveVps: (input: VpsInput) => invoke<Vps>("save_vps", { input }),
  deleteVps: (id: string) => invoke<void>("delete_vps", { id }),

  listArtifacts: (query?: string | null) =>
    invoke<Artifact[]>("list_artifacts", { query: query ?? null }),
  verifyArtifact: (id: string) => invoke<boolean>("verify_artifact", { id }),
  revealArtifact: (id: string) => invoke<void>("reveal_artifact", { id }),
  deleteArtifact: (id: string) => invoke<void>("delete_artifact", { id }),
  artifactsDir: () => invoke<string>("artifacts_dir"),

  sshConnect: (vpsId: string, cols: number, rows: number) =>
    invoke<ConnectOutcome>("ssh_connect", { vpsId, cols, rows }),
  sshWrite: (sessionId: string, dataB64: string) =>
    invoke<void>("ssh_write", { sessionId, dataB64 }),
  sshResize: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("ssh_resize", { sessionId, cols, rows }),
  sshDisconnect: (sessionId: string) =>
    invoke<void>("ssh_disconnect", { sessionId }),
  sshReplay: (sessionId: string) =>
    invoke<string | null>("ssh_replay", { sessionId }),
  /** Git status for a remote path when it is inside a repo; null otherwise. */
  remoteGitBranch: (vpsId: string, path: string) =>
    invoke<GitInfo | null>("remote_git_branch", { vpsId, path }),
  localGitBranch: (path: string) =>
    invoke<GitInfo | null>("local_git_branch", { path }),

  sftpConnect: (vpsId: string) =>
    invoke<SftpConnectOutcome>("sftp_connect", { vpsId }),
  sftpList: (sessionId: string, path: string) =>
    invoke<SftpListOutcome>("sftp_list", { sessionId, path }),
  sftpDownload: (sessionId: string, path: string) =>
    invoke<string>("sftp_download", { sessionId, path }),
  sftpWrite: (sessionId: string, path: string, contentB64: string) =>
    invoke<void>("sftp_write", { sessionId, path, contentB64 }),
  sftpMkdir: (sessionId: string, path: string) =>
    invoke<void>("sftp_mkdir", { sessionId, path }),
  sftpRename: (sessionId: string, from: string, to: string) =>
    invoke<void>("sftp_rename", { sessionId, from, to }),
  sftpRemove: (sessionId: string, path: string, isDir: boolean) =>
    invoke<void>("sftp_remove", { sessionId, path, isDir }),
  sftpSymlink: (sessionId: string, linkPath: string, target: string) =>
    invoke<void>("sftp_symlink", { sessionId, linkPath, target }),
  sftpDisconnect: (sessionId: string) =>
    invoke<void>("sftp_disconnect", { sessionId }),
  localFsList: (path?: string | null) =>
    invoke<LocalFsList>("local_fs_list", { path: path ?? null }),
  localFsHome: () => invoke<string>("local_fs_home"),

  // --- bulk transfers ---
  pickDirectory: (title: string) =>
    invoke<string | null>("pick_directory", { title }),
  pickFiles: (title: string) => invoke<string[]>("pick_files", { title }),
  pickFile: (title: string) => invoke<string | null>("pick_file", { title }),
  localFsReadText: (path: string, maxBytes?: number) =>
    invoke<string>("local_fs_read_text", { path, maxBytes: maxBytes ?? null }),
  localFsReadBytes: (path: string, maxBytes?: number) =>
    invoke<string>("local_fs_read_bytes", { path, maxBytes: maxBytes ?? null }),
  sftpTransferStart: (
    sessionId: string,
    direction: TransferDirection,
    sources: string[],
    destination: string,
    concurrency?: number,
  ) =>
    invoke<string>("sftp_transfer_start", {
      sessionId,
      direction,
      sources,
      destination,
      concurrency: concurrency ?? null,
    }),
  sftpArchiveStart: (
    sessionId: string,
    remoteDir: string,
    destination: string,
    format: ArchiveFormat,
  ) =>
    invoke<string>("sftp_archive_start", { sessionId, remoteDir, destination, format }),
  sftpTransferCancel: (id: string) => invoke<void>("sftp_transfer_cancel", { id }),
  sftpTransferList: () => invoke<TransferSnapshot[]>("sftp_transfer_list"),
  sftpTransferClearFinished: () => invoke<void>("sftp_transfer_clear_finished"),
  // --- database client ---
  dbDiscover: (vpsId: string) => invoke<DbEndpoint[]>("db_discover", { vpsId }),
  dbSaveConnection: (endpointId: string, target: DbTargetInput) =>
    invoke<string>("db_save_connection", { endpointId, target }),
  dbListConnections: (vpsId: string) =>
    invoke<DbSavedConnection[]>("db_list_connections", { vpsId }),
  dbForgetConnection: (id: string) => invoke<void>("db_forget_connection", { id }),
  dbConnectSaved: (id: string, vpsId: string) =>
    invoke<DbConnectOutcome>("db_connect_saved", { id, vpsId }),
  dbConnect: (target: DbTargetInput) =>
    invoke<DbConnectOutcome>("db_connect", { target }),
  dbDisconnect: (sessionId: string) => invoke<void>("db_disconnect", { sessionId }),
  dbUseDatabase: (sessionId: string, database: string | null) =>
    invoke<void>("db_use_database", { sessionId, database }),
  dbListDatabases: (sessionId: string) =>
    invoke<string[]>("db_list_databases", { sessionId }),
  dbListTables: (sessionId: string, schema: string) =>
    invoke<DbTable[]>("db_list_tables", { sessionId, schema }),
  dbDescribeTable: (sessionId: string, schema: string, table: string) =>
    invoke<DbColumn[]>("db_describe_table", { sessionId, schema, table }),
  dbSelectPage: (
    sessionId: string,
    schema: string,
    table: string,
    limit: number,
    offset: number,
  ) => invoke<DbResultSet>("db_select_page", { sessionId, schema, table, limit, offset }),
  dbRunSql: (sessionId: string, sql: string) =>
    invoke<DbResultSet>("db_run_sql", { sessionId, sql }),
  dbUpdateCell: (
    sessionId: string,
    schema: string,
    table: string,
    column: string,
    value: string | null,
    key: DbRowKey,
  ) =>
    invoke<DbResultSet>("db_update_cell", {
      sessionId,
      schema,
      table,
      column,
      value,
      key,
    }),
  dbDeleteRow: (sessionId: string, schema: string, table: string, key: DbRowKey) =>
    invoke<DbResultSet>("db_delete_row", { sessionId, schema, table, key }),
  /** Delete several rows in one statement — see the Rust `delete_rows_sql`. */
  dbDeleteRows: (sessionId: string, schema: string, table: string, keys: DbRowKey[]) =>
    invoke<DbResultSet>("db_delete_rows", { sessionId, schema, table, keys }),

  sftpEditExternal: (sessionId: string, path: string) =>
    invoke<{ id: string; local_path: string }>("sftp_edit_external", { sessionId, path }),

  vpsFileStat: (vpsId: string, path: string) =>
    invoke<RemoteFileStat>("vps_file_stat", { vpsId, path }),
  vpsFileChmod: (vpsId: string, path: string, mode: string, recursive: boolean) =>
    invoke<void>("vps_file_chmod", { vpsId, path, mode, recursive }),
  vpsFileChown: (
    vpsId: string,
    path: string,
    owner: string,
    group: string,
    recursive: boolean,
  ) => invoke<void>("vps_file_chown", { vpsId, path, owner, group, recursive }),
  vpsFileDelete: (vpsId: string, path: string, isDir: boolean) =>
    invoke<void>("vps_file_delete", { vpsId, path, isDir }),
  vpsFileRename: (vpsId: string, from: string, to: string) =>
    invoke<void>("vps_file_rename", { vpsId, from, to }),
  vpsFileMkdir: (vpsId: string, path: string) =>
    invoke<void>("vps_file_mkdir", { vpsId, path }),
  vpsFileTouch: (vpsId: string, path: string) =>
    invoke<void>("vps_file_touch", { vpsId, path }),
  /** Delete a selection, in one remote command. */
  vpsFileDeleteMany: (vpsId: string, paths: string[]) =>
    invoke<void>("vps_file_delete_many", { vpsId, paths }),
  /** Copy (or move) a selection into a directory, in one remote command. */
  vpsFileCopy: (
    vpsId: string,
    sources: string[],
    destDir: string,
    moveThem: boolean,
  ) => invoke<void>("vps_file_copy", { vpsId, sources, destDir, moveThem }),
  /** Search by name and/or extension, optionally through subdirectories. */
  vpsFileSearch: (
    vpsId: string,
    root: string,
    pattern: string,
    extensions: string[],
    recursive: boolean,
  ) => invoke<string[]>("vps_file_search", { vpsId, root, pattern, extensions, recursive }),
  /** Create a symlink at `path` pointing at `target`, or repoint an existing one. */
  vpsFileSymlink: (vpsId: string, path: string, target: string) =>
    invoke<void>("vps_file_symlink", { vpsId, path, target }),

  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  saveWorkspace: (input: WorkspaceInput) =>
    invoke<Workspace>("save_workspace", { input }),
  deleteWorkspace: (id: string) => invoke<void>("delete_workspace", { id }),
  reorderVps: (ids: string[]) => invoke<void>("reorder_vps", { ids }),
  getWorkspaceBrief: (id: string) =>
    invoke<string>("get_workspace_brief", { id }),
  saveWorkspaceBrief: (id: string, content: string) =>
    invoke<void>("save_workspace_brief", { id, content }),
  scanSkillPath: (path: string) =>
    invoke<SkillScanReport>("scan_skill_path", { path }),
  skillScannerStatus: () => invoke<ScannerStatus>("skill_scanner_status"),
  installSkillScanner: () => invoke<string>("install_skill_scanner"),

  getSystemCapabilities: () =>
    invoke<SystemCaps>("get_system_capabilities"),
  searchModels: (source: "ollama" | "huggingface", query: string, baseUrl?: string) =>
    invoke<ModelEntry[]>("search_models", { source, query, baseUrl: baseUrl ?? null }),
  hfModelFiles: (repoId: string) =>
    invoke<HfFile[]>("hf_model_files", { repoId }),
  downloadModel: (args: {
    source: "ollama" | "huggingface";
    id: string;
    url?: string;
    filename?: string;
    baseUrl?: string;
  }) =>
    invoke<void>("download_model", {
      source: args.source,
      id: args.id,
      url: args.url ?? null,
      filename: args.filename ?? null,
      baseUrl: args.baseUrl ?? null,
    }),
  listLocalFiles: () => invoke<LocalFile[]>("list_local_files"),
  deleteModel: (source: "ollama" | "gguf", id: string, baseUrl?: string) =>
    invoke<void>("delete_model", { source, id, baseUrl: baseUrl ?? null }),
  llamaServerStatus: () => invoke<LlamaStatus>("llama_server_status"),
  llamaServerStart: (modelFile: string, port: number, gpuLayers: number) =>
    invoke<void>("llama_server_start", { modelFile, port, gpuLayers }),
  llamaServerStop: () => invoke<void>("llama_server_stop"),
  ollamaStatus: (baseUrl?: string) =>
    invoke<OllamaStatus>("ollama_status", { baseUrl: baseUrl ?? null }),
  ollamaEnsure: (baseUrl?: string) =>
    invoke<boolean>("ollama_ensure", { baseUrl: baseUrl ?? null }),
  transcribe: (
    audioB64: string,
    engine: "local" | "cloud" | "groq" | "parakeet",
    modelFile?: string,
    lang?: string,
  ) =>
    invoke<string>("transcribe", {
      audioB64,
      engine,
      modelFile: modelFile ?? null,
      lang: lang ?? "auto",
    }),
  setupWhisper: () => invoke<string>("setup_whisper"),
  downloadWhisperModel: (modelFile: string) =>
    invoke<string>("download_whisper_model", { modelFile }),
  synthesize: (text: string, voice?: string, engine: string = "piper", instructions?: string) =>
    invoke<string>("synthesize", {
      text,
      voice: voice ?? null,
      engine,
      instructions: instructions ?? null,
    }),
  setupPiper: () => invoke<string>("setup_piper"),
  downloadPiperVoice: (voice: string) => invoke<string>("download_piper_voice", { voice }),
  setupEdgeTts: () => invoke<void>("setup_edge_tts"),
  setupParakeet: () => invoke<void>("setup_parakeet"),
  setupLlama: () => invoke<string>("setup_llama"),

  listKnownHosts: () => invoke<KnownHost[]>("list_known_hosts"),
  forgetHostKey: (host: string, port: number) =>
    invoke<void>("forget_host_key", { host, port }),

  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) =>
    invoke<void>("set_setting", { key, value }),
  listSettings: () => invoke<Setting[]>("list_settings"),
  deleteSetting: (key: string) => invoke<void>("delete_setting", { key }),

  listProviders: () => invoke<AiProvider[]>("list_providers"),
  getRemoteStatus: () => invoke<RemoteStatus>("get_remote_status"),
  saveRemoteConfig: (
    shared: {
      enabled: boolean;
      prefix: string;
      safetyMode: string;
      targets: string[];
      /** Which named agent answers. Null or "" = the unnamed main agent. */
      personaId?: string | null;
    },
    transports: {
      kind: RemoteKind;
      enabled: boolean;
      chatId: string;
      allowedUserIds: string;
      /** Null or empty keeps the stored credential; the UI is never shown it. */
      token?: string | null;
    }[],
  ) => invoke<RemoteStatus>("save_remote_config", { shared, transports }),
  clearRemoteToken: (kind: RemoteKind) =>
    invoke<RemoteStatus>("clear_remote_token", { kind }),
  /** Forget the thread every transport shares. */
  resetRemoteConversation: () => invoke<RemoteStatus>("reset_remote_conversation"),
  /** Ask the platform who a saved token belongs to. Telegram only, so far. */
  testRemoteToken: (kind: RemoteKind) => invoke<string>("test_remote_token", { kind }),

  whatsappStatus: () => invoke<WhatsAppStatus>("whatsapp_status"),
  /** Start pairing. Automatically builds the helper if needed. Progress arrives on `remote://whatsapp`. */
  whatsappLinkStart: () => invoke<WhatsAppStatus>("whatsapp_link_start"),
  whatsappLinkCancel: () => invoke<WhatsAppStatus>("whatsapp_link_cancel"),
  whatsappUnlink: () => invoke<WhatsAppStatus>("whatsapp_unlink"),
  /** Chats the WhatsApp bridge can be restricted to: your own chat, and your groups. */
  whatsappChats: () => invoke<WhatsAppChat[]>("whatsapp_chats"),
  whatsappAutoInstall: () => invoke<WhatsAppStatus>("whatsapp_auto_install"),

  listPersonas: () => invoke<Persona[]>("list_personas"),
  savePersona: (input: PersonaInput) => invoke<Persona>("save_persona", { input }),
  deletePersona: (id: string) => invoke<void>("delete_persona", { id }),
  personaOrgChart: () => invoke<string>("persona_org_chart"),
  listAgentMessages: (
    goalId?: string | null,
    /** Limit to one project. Null reads across all of them. */
    workspaceId?: string | null,
    limit?: number,
  ) =>
    invoke<AgentMessage[]>("list_agent_messages", {
      goalId: goalId ?? null,
      workspaceId: workspaceId ?? null,
      limit: limit ?? null,
    }),
  /** Messages waiting for you, from any project — each carries the project it concerns. */
  unreadUserMessages: () => invoke<AgentMessage[]>("unread_user_messages"),
  /** What one agent has done lately: tasks and outcomes, files changed, what it said. */
  agentActivity: (personaId: string, days?: number) =>
    invoke<AgentActivity>("agent_activity", { personaId, days: days ?? null }),
  /** How a project's numbers moved against the period before. */
  projectMetrics: (workspaceId: string, days?: number) =>
    invoke<MetricMovement[]>("project_metrics", { workspaceId, days: days ?? null }),
  /** One project's record: tasks, conversation, file changes and commits. */
  projectHistory: (workspaceId: string, limit?: number) =>
    invoke<ProjectHistory>("project_history", { workspaceId, limit: limit ?? null }),
  markAgentMessagesRead: (ids: string[]) =>
    invoke<void>("mark_agent_messages_read", { ids }),

  saveProvider: (input: AiProviderInput) =>
    invoke<AiProvider>("save_provider", { input }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),

  aiCliLogin: (providerId: string) =>
    invoke<string>("ai_cli_login", { providerId }),
  aiCliModels: (providerId: string) =>
    invoke<string[]>("ai_cli_models", { providerId }),
  /** Autodetect live models for any saved provider using its keychain secret. */
  aiProviderModels: (providerId: string) =>
    invoke<string[]>("ai_provider_models", { providerId }),
  /** Probe a cloud provider's /models endpoint (flavor: "openai" | "anthropic"). */
  listModels: (flavor: string, baseUrl: string, apiKey: string) =>
    invoke<string[]>("ai_list_models", { flavor, baseUrl, apiKey }),

  /** Sync model pricing from live open catalog (e.g. OpenRouter). */
  aiSyncPrices: () => invoke<number>("ai_sync_prices"),
  /** Fetch all known model pricing tables. */
  aiGetModelPrices: () => invoke<Record<string, { input: number; output: number; cache_read: number; cache_write: number }>>("ai_get_model_prices"),
  /** Set custom pricing for a model. */
  aiSetModelPrice: (args: {
    modelId: string;
    input: number;
    output: number;
    cacheRead?: number;
    cacheWrite?: number;
  }) =>
    invoke<void>("ai_set_model_price", {
      modelId: args.modelId,
      input: args.input,
      output: args.output,
      cacheRead: args.cacheRead ?? null,
      cacheWrite: args.cacheWrite ?? null,
    }),

  aiChat: (args: {
    sessionId: string;
    messages: ChatMessage[];
    providerId?: string | null;
    targets: string[];
    planMode?: boolean;
    workspaceId?: string | null;
    canvas?: CanvasSnapshotNode[];
    conversation?: boolean;
    goalId?: string | null;
  }) =>
    invoke<ChatMessage>("ai_chat", {
      sessionId: args.sessionId,
      messages: args.messages,
      providerId: args.providerId ?? null,
      targets: args.targets,
      planMode: args.planMode ?? false,
      workspaceId: args.workspaceId ?? null,
      canvas: args.canvas ?? [],
      conversation: args.conversation ?? false,
      goalId: args.goalId ?? null,
    }),

  agentCancel: (sessionId: string) => invoke<void>("agent_cancel", { sessionId }),

  // In-app updater (clone+compile): check GitHub for a newer commit, and (on accept)
  // back up data + re-run the installer's rebuild.
  checkForUpdate: () => invoke<UpdateInfo>("check_for_update"),
  startAppUpdate: () => invoke<string>("start_app_update"),
  getUpdateChannel: () => invoke<ChannelInfo>("get_update_channel"),
  setUpdateChannel: (channel: string) =>
    invoke<ChannelInfo>("set_update_channel", { channel }),

  // App lock / at-rest DB encryption.
  lockStatus: () => invoke<LockStatus>("lock_status"),
  setupLock: (password: string, remember: boolean) =>
    invoke<void>("setup_lock", { password, remember }),
  unlockWithPassword: (password: string, remember: boolean) =>
    invoke<void>("unlock_with_password", { password, remember }),
  changePassword: (oldPassword: string, newPassword: string) =>
    invoke<void>("change_password", { oldPassword, newPassword }),
  forgetDevice: () => invoke<void>("forget_device"),
  disableLock: (password: string) => invoke<void>("disable_lock", { password }),
  /** Encrypt (or decrypt) saved credentials in the OS keychain. Returns how many
   *  were converted. Forward-only for older builds — see the Rust doc comment. */
  setSecretEncryption: (enabled: boolean) =>
    invoke<number>("set_secret_encryption", { enabled }),
  /** Requires the master password whenever an app lock is configured. */
  exportUnencryptedBackup: (password: string) =>
    invoke<string>("export_unencrypted_backup", { password }),
  /** Upload dropped/pasted files to `dir` on a VPS and report where they landed.
   *  `localPaths` are paths the OS gave us; `inline` carries bytes with no path
   *  (a pasted screenshot). */
  terminalUpload: (
    vpsId: string,
    dir: string,
    localPaths: string[],
    inline: { name: string; content_b64: string }[],
  ) => invoke<Uploaded[]>("terminal_upload", { vpsId, dir, localPaths, inline }),
  /** Lock without quitting: closes every shell, re-encrypts the DB, deletes the
   *  plaintext working file, forgets the key. Resolves with how many shells closed. */
  /** Append a line to xconsole.log, for telling apart in-app and external closes. */
  logDiag: (message: string) => invoke<void>("log_diag", { message }),
  lockNow: () => invoke<number>("lock_now"),
  /** Reset the idle timer. The timeout itself is enforced in the backend. */
  noteActivity: () => invoke<void>("note_activity"),
  getAutoLockMinutes: () => invoke<number>("get_auto_lock_minutes"),
  setAutoLockMinutes: (minutes: number) =>
    invoke<void>("set_auto_lock_minutes", { minutes }),

  listFileChanges: (sessionId: string) =>
    invoke<FileChange[]>("list_file_changes", { sessionId }),
  listFileChangesHistory: (workspaceId?: string | null, sessionId?: string | null) =>
    invoke<FileChange[]>("list_file_changes_history", {
      workspaceId: workspaceId ?? null,
      sessionId: sessionId ?? null,
    }),
  clearFileChanges: (sessionId: string) =>
    invoke<void>("clear_file_changes", { sessionId }),
  revertFileChange: (id: string) => invoke<void>("revert_file_change", { id }),

  listPlans: (sessionId?: string | null, workspaceId?: string | null) =>
    invoke<AgentPlanMeta[]>("list_plans", {
      sessionId: sessionId ?? null,
      workspaceId: workspaceId ?? null,
    }),
  getPlan: (id: string) => invoke<AgentPlanFull | null>("get_plan", { id }),
  archivePlan: (id: string) => invoke<void>("archive_plan", { id }),
  cancelPlan: (id: string) => invoke<void>("cancel_plan", { id }),

  agentResolveApproval: (
    id: string,
    approved: boolean,
    remember?: boolean,
    sessionId?: string,
  ) =>
    invoke<void>("agent_resolve_approval", {
      id,
      approved,
      remember: remember ?? false,
      sessionId: sessionId ?? null,
    }),
  /** Answer a pending ask_user question or a present_plan decision. */
  agentAnswerPrompt: (id: string, answer: string) =>
    invoke<void>("agent_answer_prompt", { id, answer }),
  listPendingApprovals: () =>
    invoke<AgentApproval[]>("list_pending_approvals"),

  listAgentConversations: () =>
    invoke<AgentConversationMeta[]>("list_agent_conversations"),
  agentAnalytics: () => invoke<AgentAnalytics>("agent_analytics"),
  appResourceSnapshot: () => invoke<ResourceSnapshot>("app_resource_snapshot"),
  getAgentConversation: (id: string) =>
    invoke<AgentConversation | null>("get_agent_conversation", { id }),
  saveAgentConversation: (args: {
    id: string;
    title?: string | null;
    targets: string[];
    messagesJson: string;
  }) =>
    invoke<AgentConversation>("save_agent_conversation", {
      input: {
        id: args.id,
        title: args.title ?? null,
        targets: args.targets,
        messages_json: args.messagesJson,
      },
    }),
  deleteAgentConversation: (id: string) =>
    invoke<void>("delete_agent_conversation", { id }),

  getAgentDocs: () => invoke<AgentDocs>("get_agent_docs"),
  saveSoul: (content: string) => invoke<void>("save_soul", { content }),

  getHooksConfig: () => invoke<string>("get_hooks_config"),
  saveHooksConfig: (content: string) =>
    invoke<number>("save_hooks_config", { content }),
  reloadHooks: () => invoke<number>("reload_hooks"),
  hooksStatus: () => invoke<HooksStatus>("hooks_status"),
  saveMemoryDoc: (content: string) =>
    invoke<void>("save_memory_doc", { content }),
  saveTasteDoc: (content: string) => invoke<void>("save_taste_doc", { content }),

  listSkills: () => invoke<Skill[]>("list_skills"),
  getSkill: (category: string, name: string) =>
    invoke<string | null>("get_skill", { category, name }),
  saveSkill: (category: string, name: string, content: string) =>
    invoke<void>("save_skill", { category, name, content }),
  deleteSkill: (category: string, name: string) =>
    invoke<void>("delete_skill", { category, name }),

  listCronJobs: () => invoke<CronJob[]>("list_cron_jobs"),
  saveCronJob: (input: CronJobInput) =>
    invoke<CronJob>("save_cron_job", { input }),
  deleteCronJob: (id: string) => invoke<void>("delete_cron_job", { id }),
  runCronJob: (id: string) => invoke<void>("run_cron_job", { id }),

  startGoal: (text: string, workspaceId?: string | null) =>
    invoke<string>("start_goal", { text, workspaceId: workspaceId ?? null }),
  confirmGoal: (id: string, targets?: string[]) =>
    invoke<void>("confirm_goal", { id, targets: targets ?? [] }),
  pauseGoal: (id: string) => invoke<void>("pause_goal", { id }),
  continueGoal: (id: string) => invoke<void>("continue_goal", { id }),
  stopGoal: (id: string) => invoke<void>("stop_goal", { id }),
  getGoal: (id: string) => invoke<GoalSession>("get_goal", { id }),
  listGoals: () => invoke<GoalSession[]>("list_goals"),
  deleteGoal: (id: string) => invoke<void>("delete_goal", { id }),

  listInfraProjects: () => invoke<InfraProject[]>("list_infra_projects"),
  saveInfraProject: (input: InfraProjectInput) =>
    invoke<InfraProject>("save_infra_project", { input }),
  deleteInfraProject: (id: string) => invoke<void>("delete_infra_project", { id }),
  getInfraProject: (id: string) =>
    invoke<InfraProject | null>("get_infra_project", { id }),
  readProjectFile: (slug: string, path: string) =>
    invoke<string>("read_project_file_cmd", { slug, path }),

  listCloudAccounts: () => invoke<CloudAccount[]>("list_cloud_accounts"),
  saveCloudAccount: (input: CloudAccountInput) =>
    invoke<CloudAccount>("save_cloud_account", { input }),
  deleteCloudAccount: (id: string) => invoke<void>("delete_cloud_account", { id }),
  listTfcWorkspaces: (accountId: string) =>
    invoke<string[]>("list_tfc_workspaces", { accountId }),
  listCloudResources: (accountId: string, resource?: string) =>
    invoke<string>("list_cloud_resources", { accountId, resource }),

  // Cloudflare
  startCloudflareOAuthLogin: () =>
    invoke<string>("start_cloudflare_oauth_login"),
  saveCloudflareManualToken: (token: string) =>
    invoke<CloudAccount>("save_cloudflare_manual_token", { token }),
  listCloudflareZones: (accountId: string) =>
    invoke<CloudflareZone[]>("list_cloudflare_zones", { accountId }),
  listCloudflareTunnels: (accountId: string) =>
    invoke<CloudflareTunnel[]>("list_cloudflare_tunnels", { accountId }),
  createCloudflareTunnel: (accountId: string, name: string) =>
    invoke<CloudflareTunnel>("create_cloudflare_tunnel", { accountId, name }),
  deleteCloudflareTunnel: (accountId: string, tunnelId: string) =>
    invoke<void>("delete_cloudflare_tunnel", { accountId, tunnelId }),
  getCloudflareTunnelConfig: (accountId: string, tunnelId: string) =>
    invoke<CloudflareTunnelConfig>("get_cloudflare_tunnel_config", { accountId, tunnelId }),
  saveCloudflareTunnelConfig: (accountId: string, tunnelId: string, config: CloudflareTunnelConfig) =>
    invoke<CloudflareTunnelConfig>("save_cloudflare_tunnel_config", { accountId, tunnelId, config }),
  getCloudflareTunnelToken: (accountId: string, tunnelId: string) =>
    invoke<string>("get_cloudflare_tunnel_token", { accountId, tunnelId }),
  listCloudflareDnsRecords: (accountId: string, zoneId: string) =>
    invoke<CloudflareDnsRecord[]>("list_cloudflare_dns_records", { accountId, zoneId }),
  upsertCloudflareDnsRecord: (accountId: string, zoneId: string, record: CloudflareDnsRecordInput) =>
    invoke<CloudflareDnsRecord>("upsert_cloudflare_dns_record", { accountId, zoneId, record }),
  deleteCloudflareDnsRecord: (accountId: string, zoneId: string, recordId: string) =>
    invoke<void>("delete_cloudflare_dns_record", { accountId, zoneId, recordId }),
  getCloudflareSecuritySettings: (accountId: string, zoneId: string) =>
    invoke<CloudflareSecuritySettings>("get_cloudflare_security_settings", { accountId, zoneId }),
  setCloudflareSecurityLevel: (accountId: string, zoneId: string, level: string) =>
    invoke<string>("set_cloudflare_security_level", { accountId, zoneId, level }),
  listCloudflareHistory: (accountId: string) =>
    invoke<CloudflareAuditLog[]>("list_cloudflare_history", { accountId }),
  revertCloudflareAction: (accountId: string, logId: string) =>
    invoke<string>("revert_cloudflare_action", { accountId, logId }),

  // Plugin Harness (DeepSeek Harness / Cordis paradigm)
  listInstalledPlugins: () =>
    invoke<PluginManifest[]>("list_installed_plugins"),
  getDisabledPluginIds: () =>
    invoke<string[]>("get_disabled_plugin_ids_cmd"),
  getPluginReadme: (pluginId: string) =>
    invoke<string>("get_plugin_readme_cmd", { pluginId }),
  installPlugin: (source: string) =>
    invoke<PluginManifest>("install_plugin_cmd", { source }),
  linkPlugin: (path: string) =>
    invoke<PluginManifest>("link_plugin_cmd", { path }),
  uninstallPlugin: (pluginId: string) =>
    invoke<void>("uninstall_plugin_cmd", { pluginId }),
  togglePlugin: (pluginId: string, enabled: boolean) =>
    invoke<boolean>("toggle_plugin_cmd", { pluginId, enabled }),
  reloadPlugins: () =>
    invoke<PluginManifest[]>("reload_plugins_cmd"),
  checkPluginUpdates: () =>
    invoke<PluginUpdateInfo[]>("check_plugin_updates_cmd"),
  checkSinglePluginUpdate: (pluginId: string) =>
    invoke<PluginUpdateInfo>("check_single_plugin_update_cmd", { pluginId }),
  updatePlugin: (pluginId: string) =>
    invoke<PluginManifest>("update_plugin_cmd", { pluginId }),
  updateAllPlugins: () =>
    invoke<PluginManifest[]>("update_all_plugins_cmd"),
  setPluginRemote: (pluginId: string, remoteUrl: string) =>
    invoke<string>("set_plugin_remote_cmd", { pluginId, remoteUrl }),
};

/** Subscribe to streamed output from a CLI provider's login flow. */
export function onAiLoginOutput(
  providerId: string,
  cb: (ev: StreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<StreamEvent>(`ai://login/${providerId}`, (e) => cb(e.payload));
}

/** Subscribe to a chat session's streamed agent output. */
export function onAiChatOutput(
  sessionId: string,
  cb: (ev: StreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<StreamEvent>(`ai://chat/${sessionId}`, (e) => cb(e.payload));
}

/** Subscribe to pending command-approval requests from the agent. */
export function onAgentApproval(
  cb: (approval: AgentApproval) => void,
): Promise<UnlistenFn> {
  return listen<AgentApproval>("ai://approval", (e) => cb(e.payload));
}

/** Subscribe to clarifying questions the agent asks (ask_user tool). */
export function onAgentQuestion(
  cb: (question: AgentQuestion) => void,
): Promise<UnlistenFn> {
  return listen<AgentQuestion>("ai://question", (e) => cb(e.payload));
}

/** Subscribe to plans the agent presents for approval (present_plan tool). */
export function onAgentPlan(cb: (plan: AgentPlan) => void): Promise<UnlistenFn> {
  return listen<AgentPlan>("ai://plan", (e) => cb(e.payload));
}

/** A canvas action requested by the agent (open/close a node, tile). */
export interface CanvasCommand {
  action: "open_terminal" | "open_sftp" | "tile" | "close" | "reconnect";
  vps_id?: string;
  /** Target one specific canvas panel (close/reconnect). */
  node_id?: string;
}

/** A snapshot of one open canvas node, sent to the agent each turn so it can see
 * the user's live terminals / SFTP panels. Field names are snake_case to match
 * the Rust `CanvasNode` deserializer. */
export interface CanvasSnapshotNode {
  /** The Rust side matches on this with a catch-all arm, so a kind it doesn't render
   *  yet (like "db") is ignored rather than mislabelled. */
  kind: "terminal" | "sftp" | "db";
  /** Canvas node id, so the agent can target one specific panel. */
  node_id: string;
  vps_id: string;
  name: string;
  host: string;
  /** Backend SSH session id (terminals) — lets the agent read live scrollback. */
  session_id?: string;
  status?: string;
  /** Terminal working directory. */
  cwd?: string;
  /** SFTP panel's current remote path. */
  path?: string;
  /** Git branch when path/cwd is inside a repo. */
  git_branch?: string | null;
  /** Uncommitted changes. */
  git_dirty?: boolean | null;
}

/** Subscribe to canvas actions the agent requests (drive the live canvas). */
export function onCanvasCommand(
  cb: (cmd: CanvasCommand) => void,
): Promise<UnlistenFn> {
  return listen<CanvasCommand>("canvas://command", (e) => cb(e.payload));
}

export interface CanvasPreviewPayload {
  id: string;
  title: string;
  html: string;
  width: number;
  height: number;
}

/** Subscribe to live HTML/design sandbox preview requests from the agent. */
export function onCanvasPreview(
  cb: (payload: CanvasPreviewPayload) => void,
): Promise<UnlistenFn> {
  return listen<CanvasPreviewPayload>("canvas://open-preview", (e) => cb(e.payload));
}

export function onVpsUpdated(cb: () => void): Promise<UnlistenFn> {
  return listen("vps://updated", () => cb());
}

export function onArtifactsChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("artifacts://changed", () => cb());
}

/** Subscribe to live goal-session events (kanban/status/memory updates). */
export function onGoalEvent(
  goalId: string,
  cb: (ev: StreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<StreamEvent>(`goal://${goalId}`, (e) => cb(e.payload));
}

/** One file the agent edited this session (before/after captured for the diff panel). */
/** App-lock status (matches the Rust `LockStatus`). */
/** A file that was uploaded to a server by dropping or pasting it on a terminal. */
export interface Uploaded {
  name: string;
  path: string;
  size: number;
  preview_b64: string | null;
  is_image: boolean;
}

export interface LockStatus {
  enabled: boolean;
  unlocked: boolean;
  remembered: boolean;
  /** Saved SSH/API credentials are encrypted in the OS keychain with the data key. */
  secrets_encrypted: boolean;
}

/** Result of the in-app update check (matches the Rust `UpdateInfo`). */
export interface UpdateInfo {
  available: boolean;
  current: string | null;
  latest: string | null;
  message: string;
  date: string;
  can_self_update: boolean;
  note: string | null;
  /** Active update channel: `main` (stable) or `dev`. */
  channel: string;
  /** Local checkout branch, when known. */
  local_branch: string | null;
}

/** Active channel + local build identity (matches Rust `ChannelInfo`). */
export interface ChannelInfo {
  channel: string;
  local_branch: string | null;
  current: string | null;
  can_self_update: boolean;
}

export interface FileChange {
  id: string;
  session_id: string;
  workspace_id?: string | null;
  scope: "local" | "vps";
  vps_id?: string | null;
  label: string;
  path: string;
  before: string;
  after: string;
  is_new: boolean;
  reverted: boolean;
  ts: number;
}

/** Lifecycle of an external-editor session (matches the Rust `ExternalEditEvent`). */
export type ExternalEditEvent =
  | { kind: "opened"; id: string; remote_path: string; local_path: string }
  | { kind: "saved"; id: string; remote_path: string; bytes: number }
  | { kind: "skipped"; id: string; remote_path: string; reason: string }
  | { kind: "failed"; id: string; remote_path: string; error: string }
  | { kind: "closed"; id: string; remote_path: string };

/** Fired when a file open in an external editor is saved back (or refused). */
export function onExternalEdit(
  cb: (e: ExternalEditEvent) => void,
): Promise<UnlistenFn> {
  return listen<ExternalEditEvent>("sftp://external-edit", (e) => cb(e.payload));
}

/** Fired as an SFTP transfer progresses. Each event is a full job snapshot. */
export function onTransferProgress(
  cb: (t: TransferSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<TransferSnapshot>("sftp://transfer", (e) => cb(e.payload));
}

/** Fired when the agent edits a file. */
export function onFileChange(cb: (c: FileChange) => void): Promise<UnlistenFn> {
  return listen<FileChange>("agent://file-change", (e) => cb(e.payload));
}

/** Fired when an edit is reverted (payload is the change id). */
export function onFileChangeReverted(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("agent://file-change-reverted", (e) => cb(e.payload));
}

/** Per-workspace agent status (working / planning / testing / idle). */
export interface AgentWorkspaceStatus {
  workspace_id: string;
  status: string;
}

export function onAgentWorkspaceStatus(
  cb: (s: AgentWorkspaceStatus) => void,
): Promise<UnlistenFn> {
  return listen<AgentWorkspaceStatus>("agent://workspace-status", (e) =>
    cb(e.payload),
  );
}

/** Subscribe to model-download progress. */
export function onModelDownload(
  cb: (p: DownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<DownloadProgress>("models://download", (e) => cb(e.payload));
}

export interface PluginUpdateInfo {
  plugin_id: string;
  plugin_name: string;
  current_version: string;
  new_version?: string | null;
  current_commit: string;
  latest_commit: string;
  repository_url: string;
  has_update: boolean;
  is_git_repo: boolean;
  commit_message?: string | null;
}

export interface PluginInstallProgress {
  step: string;
  step_index: number;
  total_steps: number;
  percent: number;
  log_line?: string | null;
  is_error: boolean;
  is_done: boolean;
}

/** Subscribe to real-time plugin installation progress and logs. */
export function onPluginInstallProgress(
  cb: (p: PluginInstallProgress) => void,
): Promise<UnlistenFn> {
  return listen<PluginInstallProgress>("plugins://install-progress", (e) => cb(e.payload));
}

/** Subscribe to a session's terminal output (base64-encoded chunks). */
export function onSessionOutput(
  sessionId: string,
  cb: (bytes: Uint8Array) => void,
): Promise<UnlistenFn> {
  return listen<string>(`ssh://${sessionId}/output`, (e) => {
    cb(b64ToBytes(e.payload));
  });
}

/** Live feed of what the agents say to each other. */
export function onAgentMessage(
  cb: (msg: AgentMessage) => void,
): Promise<UnlistenFn> {
  return listen<AgentMessage>("agent://message", (e) => cb(e.payload));
}

/** Subscribe to a session's connection status changes. */
export function onSessionStatus(
  sessionId: string,
  cb: (status: SessionStatus) => void,
): Promise<UnlistenFn> {
  return listen<SessionStatus>(`ssh://${sessionId}/status`, (e) => cb(e.payload));
}

export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function bytesToB64(data: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < data.length; i++) bin += String.fromCharCode(data[i]);
  return btoa(bin);
}

export function strToB64(s: string): string {
  return bytesToB64(new TextEncoder().encode(s));
}

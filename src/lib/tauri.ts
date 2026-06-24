import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  is_dir: boolean;
  size: number;
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
  | "opencode_cli";

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
  title?: string;
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
  user: string;
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

export type CloudKind = "aws" | "gcp" | "tfc";

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

export interface ToolCall {
  id: string;
  name: string;
  arguments: unknown;
}

export type ChatRole = "user" | "assistant" | "tool" | "system";

export interface ChatMessage {
  role: ChatRole;
  content: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string | null;
  activity?: AgentActivityItem[];
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
        duration_ms: number;
        tokens_per_sec: number;
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

  sftpConnect: (vpsId: string) =>
    invoke<SftpConnectOutcome>("sftp_connect", { vpsId }),
  sftpList: (sessionId: string, path: string) =>
    invoke<SftpListOutcome>("sftp_list", { sessionId, path }),
  sftpDownload: (sessionId: string, path: string) =>
    invoke<string>("sftp_download", { sessionId, path }),
  sftpWrite: (sessionId: string, path: string, contentB64: string) =>
    invoke<void>("sftp_write", { sessionId, path, contentB64 }),
  sftpDisconnect: (sessionId: string) =>
    invoke<void>("sftp_disconnect", { sessionId }),

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
  saveProvider: (input: AiProviderInput) =>
    invoke<AiProvider>("save_provider", { input }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),

  aiCliLogin: (providerId: string) =>
    invoke<string>("ai_cli_login", { providerId }),

  aiChat: (args: {
    sessionId: string;
    messages: ChatMessage[];
    providerId?: string | null;
    targets: string[];
    planMode?: boolean;
    workspaceId?: string | null;
    canvas?: CanvasSnapshotNode[];
  }) =>
    invoke<ChatMessage>("ai_chat", {
      sessionId: args.sessionId,
      messages: args.messages,
      providerId: args.providerId ?? null,
      targets: args.targets,
      planMode: args.planMode ?? false,
      workspaceId: args.workspaceId ?? null,
      canvas: args.canvas ?? [],
    }),

  agentCancel: (sessionId: string) => invoke<void>("agent_cancel", { sessionId }),

  listFileChanges: (sessionId: string) =>
    invoke<FileChange[]>("list_file_changes", { sessionId }),
  clearFileChanges: (sessionId: string) =>
    invoke<void>("clear_file_changes", { sessionId }),
  revertFileChange: (id: string) => invoke<void>("revert_file_change", { id }),

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
  saveMemoryDoc: (content: string) =>
    invoke<void>("save_memory_doc", { content }),
  saveUserDoc: (content: string) => invoke<void>("save_user_doc", { content }),

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
  kind: "terminal" | "sftp";
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
}

/** Subscribe to canvas actions the agent requests (drive the live canvas). */
export function onCanvasCommand(
  cb: (cmd: CanvasCommand) => void,
): Promise<UnlistenFn> {
  return listen<CanvasCommand>("canvas://command", (e) => cb(e.payload));
}

/** One file the agent edited this session (before/after captured for the diff panel). */
export interface FileChange {
  id: string;
  session_id: string;
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

/** Subscribe to a session's terminal output (base64-encoded chunks). */
export function onSessionOutput(
  sessionId: string,
  cb: (bytes: Uint8Array) => void,
): Promise<UnlistenFn> {
  return listen<string>(`ssh://${sessionId}/output`, (e) => {
    cb(b64ToBytes(e.payload));
  });
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

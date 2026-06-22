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
  | { kind: "ConversationCompacted"; data: ChatMessage[] }
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
  }) =>
    invoke<ChatMessage>("ai_chat", {
      sessionId: args.sessionId,
      messages: args.messages,
      providerId: args.providerId ?? null,
      targets: args.targets,
    }),

  agentResolveApproval: (id: string, approved: boolean) =>
    invoke<void>("agent_resolve_approval", { id, approved }),
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

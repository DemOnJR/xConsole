import { type UnlistenFn } from "@tauri-apps/api/event";
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
    cache: {
        ts: string;
        session: string;
        prompt: number;
        hit: number;
        miss: number;
        pct: number;
    }[];
    cache_avg_pct: number;
    conversations: {
        id: string;
        title: string;
        updated_at: string;
        user_turns: number;
        tool_calls: number;
        tools: {
            name: string;
            count: number;
        }[];
    }[];
    tools_all: {
        name: string;
        count: number;
    }[];
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
/** An engine the client can actually drive. */
export type DbEngine = "mysql" | "postgres" | "redis";
/** Any database product discovery can recognise, browsable or not. */
export type DbProduct = "mysql" | "postgres" | "mongodb" | "redis" | "mssql" | "clickhouse" | "cassandra" | "couchdb" | "elasticsearch";
/** Human label for a product, for the tree. */
export declare const DB_PRODUCT_LABEL: Record<DbProduct, string>;
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
export type TransferJobState = "scanning" | "running" | "done" | "failed" | "cancelled";
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
export type ProviderKind = "anthropic" | "openai" | "ollama" | "llamacpp" | "cursor" | "codex_cli" | "opencode_cli" | "antigravity_cli";
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
    learned: {
        key: string;
        value: string;
        evidence: string;
        confidence: string;
    }[];
    history?: {
        at: string;
        action: string;
        next_check: string;
    }[];
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
export type ActivityEvent = {
    type: "ToolStart";
    data: {
        id: string;
        tool: string;
        label: string;
        detail?: string;
    };
} | {
    type: "ToolEnd";
    data: {
        id: string;
        ok: boolean;
    };
} | {
    type: "SkillRead";
    data: {
        id: string;
        category: string;
        name: string;
    };
} | {
    type: "SkillSaved";
    data: {
        id: string;
        category: string;
        name: string;
    };
} | {
    type: "Command";
    data: {
        id: string;
        vps: string;
        command: string;
    };
} | {
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
export type StreamEvent = {
    kind: "Text";
    data: string;
} | {
    kind: "Status";
    data: string;
} | {
    kind: "ToolCall";
    data: ToolCall;
} | {
    kind: "ToolResult";
    data: {
        id: string;
        output: string;
    };
} | {
    kind: "Activity";
    data: ActivityEvent;
} | {
    kind: "Stats";
    data: {
        completion_tokens: number;
        prompt_tokens?: number | null;
        cached_tokens?: number | null;
        cache_creation_tokens?: number | null;
        duration_ms: number;
        tokens_per_sec: number;
    };
} | {
    kind: "Cost";
    data: {
        input_tokens: number;
        output_tokens: number;
        cache_read_tokens: number;
        cache_write_tokens: number;
        usd: number;
    };
} | {
    kind: "TurnTelemetry";
    data: {
        tool_calls: number;
        tool_cache_lookups: number;
        tool_cache_hits: number;
        tool_cache_misses: number;
        tool_cache_writes: number;
        tool_cache_hit_rate: number;
    };
} | {
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
} | {
    kind: "ContextUsage";
    data: {
        segments: {
            key: string;
            label: string;
            tokens: number;
        }[];
        total_tokens: number;
        context_limit: number;
        percent: number;
    };
} | {
    kind: "ConversationCompacted";
    data: {
        messages: ChatMessage[];
    };
} | {
    kind: "Done";
} | {
    kind: "Error";
    data: string;
};
export type SessionStatus = {
    kind: "Connecting";
} | {
    kind: "Connected";
} | {
    kind: "Reconnecting";
} | {
    kind: "Disconnected";
} | {
    kind: "Error";
    detail: string;
};
export declare const api: {
    listVps: () => Promise<Vps[]>;
    saveVps: (input: VpsInput) => Promise<Vps>;
    deleteVps: (id: string) => Promise<void>;
    listArtifacts: (query?: string | null) => Promise<Artifact[]>;
    verifyArtifact: (id: string) => Promise<boolean>;
    revealArtifact: (id: string) => Promise<void>;
    deleteArtifact: (id: string) => Promise<void>;
    artifactsDir: () => Promise<string>;
    sshConnect: (vpsId: string, cols: number, rows: number) => Promise<ConnectOutcome>;
    sshWrite: (sessionId: string, dataB64: string) => Promise<void>;
    sshResize: (sessionId: string, cols: number, rows: number) => Promise<void>;
    sshDisconnect: (sessionId: string) => Promise<void>;
    sshReplay: (sessionId: string) => Promise<string | null>;
    /** Git status for a remote path when it is inside a repo; null otherwise. */
    remoteGitBranch: (vpsId: string, path: string) => Promise<GitInfo | null>;
    localGitBranch: (path: string) => Promise<GitInfo | null>;
    sftpConnect: (vpsId: string) => Promise<SftpConnectOutcome>;
    sftpList: (sessionId: string, path: string) => Promise<SftpListOutcome>;
    sftpDownload: (sessionId: string, path: string) => Promise<string>;
    sftpWrite: (sessionId: string, path: string, contentB64: string) => Promise<void>;
    sftpMkdir: (sessionId: string, path: string) => Promise<void>;
    sftpRename: (sessionId: string, from: string, to: string) => Promise<void>;
    sftpRemove: (sessionId: string, path: string, isDir: boolean) => Promise<void>;
    sftpSymlink: (sessionId: string, linkPath: string, target: string) => Promise<void>;
    sftpDisconnect: (sessionId: string) => Promise<void>;
    localFsList: (path?: string | null) => Promise<LocalFsList>;
    localFsHome: () => Promise<string>;
    pickDirectory: (title: string) => Promise<string | null>;
    pickFiles: (title: string) => Promise<string[]>;
    pickFile: (title: string) => Promise<string | null>;
    localFsReadText: (path: string, maxBytes?: number) => Promise<string>;
    localFsReadBytes: (path: string, maxBytes?: number) => Promise<string>;
    sftpTransferStart: (sessionId: string, direction: TransferDirection, sources: string[], destination: string, concurrency?: number) => Promise<string>;
    sftpArchiveStart: (sessionId: string, remoteDir: string, destination: string, format: ArchiveFormat) => Promise<string>;
    sftpTransferCancel: (id: string) => Promise<void>;
    sftpTransferList: () => Promise<TransferSnapshot[]>;
    sftpTransferClearFinished: () => Promise<void>;
    dbDiscover: (vpsId: string) => Promise<DbEndpoint[]>;
    dbSaveConnection: (endpointId: string, target: DbTargetInput) => Promise<string>;
    dbListConnections: (vpsId: string) => Promise<DbSavedConnection[]>;
    dbForgetConnection: (id: string) => Promise<void>;
    dbConnectSaved: (id: string, vpsId: string) => Promise<DbConnectOutcome>;
    dbConnect: (target: DbTargetInput) => Promise<DbConnectOutcome>;
    dbDisconnect: (sessionId: string) => Promise<void>;
    dbUseDatabase: (sessionId: string, database: string | null) => Promise<void>;
    dbListDatabases: (sessionId: string) => Promise<string[]>;
    dbListTables: (sessionId: string, schema: string) => Promise<DbTable[]>;
    dbDescribeTable: (sessionId: string, schema: string, table: string) => Promise<DbColumn[]>;
    dbSelectPage: (sessionId: string, schema: string, table: string, limit: number, offset: number) => Promise<DbResultSet>;
    dbRunSql: (sessionId: string, sql: string) => Promise<DbResultSet>;
    dbUpdateCell: (sessionId: string, schema: string, table: string, column: string, value: string | null, key: DbRowKey) => Promise<DbResultSet>;
    dbDeleteRow: (sessionId: string, schema: string, table: string, key: DbRowKey) => Promise<DbResultSet>;
    /** Delete several rows in one statement — see the Rust `delete_rows_sql`. */
    dbDeleteRows: (sessionId: string, schema: string, table: string, keys: DbRowKey[]) => Promise<DbResultSet>;
    sftpEditExternal: (sessionId: string, path: string) => Promise<{
        id: string;
        local_path: string;
    }>;
    vpsFileStat: (vpsId: string, path: string) => Promise<RemoteFileStat>;
    vpsFileChmod: (vpsId: string, path: string, mode: string, recursive: boolean) => Promise<void>;
    vpsFileChown: (vpsId: string, path: string, owner: string, group: string, recursive: boolean) => Promise<void>;
    vpsFileDelete: (vpsId: string, path: string, isDir: boolean) => Promise<void>;
    vpsFileRename: (vpsId: string, from: string, to: string) => Promise<void>;
    vpsFileMkdir: (vpsId: string, path: string) => Promise<void>;
    vpsFileTouch: (vpsId: string, path: string) => Promise<void>;
    /** Delete a selection, in one remote command. */
    vpsFileDeleteMany: (vpsId: string, paths: string[]) => Promise<void>;
    /** Copy (or move) a selection into a directory, in one remote command. */
    vpsFileCopy: (vpsId: string, sources: string[], destDir: string, moveThem: boolean) => Promise<void>;
    /** Search by name and/or extension, optionally through subdirectories. */
    vpsFileSearch: (vpsId: string, root: string, pattern: string, extensions: string[], recursive: boolean) => Promise<string[]>;
    /** Create a symlink at `path` pointing at `target`, or repoint an existing one. */
    vpsFileSymlink: (vpsId: string, path: string, target: string) => Promise<void>;
    listWorkspaces: () => Promise<Workspace[]>;
    saveWorkspace: (input: WorkspaceInput) => Promise<Workspace>;
    deleteWorkspace: (id: string) => Promise<void>;
    reorderVps: (ids: string[]) => Promise<void>;
    getWorkspaceBrief: (id: string) => Promise<string>;
    saveWorkspaceBrief: (id: string, content: string) => Promise<void>;
    scanSkillPath: (path: string) => Promise<SkillScanReport>;
    skillScannerStatus: () => Promise<ScannerStatus>;
    installSkillScanner: () => Promise<string>;
    getSystemCapabilities: () => Promise<SystemCaps>;
    searchModels: (source: "ollama" | "huggingface", query: string, baseUrl?: string) => Promise<ModelEntry[]>;
    hfModelFiles: (repoId: string) => Promise<HfFile[]>;
    downloadModel: (args: {
        source: "ollama" | "huggingface";
        id: string;
        url?: string;
        filename?: string;
        baseUrl?: string;
    }) => Promise<void>;
    listLocalFiles: () => Promise<LocalFile[]>;
    deleteModel: (source: "ollama" | "gguf", id: string, baseUrl?: string) => Promise<void>;
    llamaServerStatus: () => Promise<LlamaStatus>;
    llamaServerStart: (modelFile: string, port: number, gpuLayers: number) => Promise<void>;
    llamaServerStop: () => Promise<void>;
    ollamaStatus: (baseUrl?: string) => Promise<OllamaStatus>;
    ollamaEnsure: (baseUrl?: string) => Promise<boolean>;
    transcribe: (audioB64: string, engine: "local" | "cloud" | "groq" | "parakeet", modelFile?: string, lang?: string) => Promise<string>;
    setupWhisper: () => Promise<string>;
    downloadWhisperModel: (modelFile: string) => Promise<string>;
    synthesize: (text: string, voice?: string, engine?: string, instructions?: string) => Promise<string>;
    setupPiper: () => Promise<string>;
    downloadPiperVoice: (voice: string) => Promise<string>;
    setupEdgeTts: () => Promise<void>;
    setupParakeet: () => Promise<void>;
    setupLlama: () => Promise<string>;
    listKnownHosts: () => Promise<KnownHost[]>;
    forgetHostKey: (host: string, port: number) => Promise<void>;
    getSetting: (key: string) => Promise<string | null>;
    setSetting: (key: string, value: string) => Promise<void>;
    listSettings: () => Promise<Setting[]>;
    deleteSetting: (key: string) => Promise<void>;
    listProviders: () => Promise<AiProvider[]>;
    saveProvider: (input: AiProviderInput) => Promise<AiProvider>;
    deleteProvider: (id: string) => Promise<void>;
    aiCliLogin: (providerId: string) => Promise<string>;
    aiCliModels: (providerId: string) => Promise<string[]>;
    /** Autodetect live models for any saved provider using its keychain secret. */
    aiProviderModels: (providerId: string) => Promise<string[]>;
    /** Probe a cloud provider's /models endpoint (flavor: "openai" | "anthropic"). */
    listModels: (flavor: string, baseUrl: string, apiKey: string) => Promise<string[]>;
    /** Sync model pricing from live open catalog (e.g. OpenRouter). */
    aiSyncPrices: () => Promise<number>;
    /** Fetch all known model pricing tables. */
    aiGetModelPrices: () => Promise<Record<string, {
        input: number;
        output: number;
        cache_read: number;
        cache_write: number;
    }>>;
    /** Set custom pricing for a model. */
    aiSetModelPrice: (args: {
        modelId: string;
        input: number;
        output: number;
        cacheRead?: number;
        cacheWrite?: number;
    }) => Promise<void>;
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
    }) => Promise<ChatMessage>;
    agentCancel: (sessionId: string) => Promise<void>;
    checkForUpdate: () => Promise<UpdateInfo>;
    startAppUpdate: () => Promise<string>;
    getUpdateChannel: () => Promise<ChannelInfo>;
    setUpdateChannel: (channel: string) => Promise<ChannelInfo>;
    lockStatus: () => Promise<LockStatus>;
    setupLock: (password: string, remember: boolean) => Promise<void>;
    unlockWithPassword: (password: string, remember: boolean) => Promise<void>;
    changePassword: (oldPassword: string, newPassword: string) => Promise<void>;
    forgetDevice: () => Promise<void>;
    disableLock: (password: string) => Promise<void>;
    /** Encrypt (or decrypt) saved credentials in the OS keychain. Returns how many
     *  were converted. Forward-only for older builds — see the Rust doc comment. */
    setSecretEncryption: (enabled: boolean) => Promise<number>;
    /** Requires the master password whenever an app lock is configured. */
    exportUnencryptedBackup: (password: string) => Promise<string>;
    /** Upload dropped/pasted files to `dir` on a VPS and report where they landed.
     *  `localPaths` are paths the OS gave us; `inline` carries bytes with no path
     *  (a pasted screenshot). */
    terminalUpload: (vpsId: string, dir: string, localPaths: string[], inline: {
        name: string;
        content_b64: string;
    }[]) => Promise<Uploaded[]>;
    /** Lock without quitting: closes every shell, re-encrypts the DB, deletes the
     *  plaintext working file, forgets the key. Resolves with how many shells closed. */
    /** Append a line to xconsole.log, for telling apart in-app and external closes. */
    logDiag: (message: string) => Promise<void>;
    lockNow: () => Promise<number>;
    /** Reset the idle timer. The timeout itself is enforced in the backend. */
    noteActivity: () => Promise<void>;
    getAutoLockMinutes: () => Promise<number>;
    setAutoLockMinutes: (minutes: number) => Promise<void>;
    listFileChanges: (sessionId: string) => Promise<FileChange[]>;
    listFileChangesHistory: (workspaceId?: string | null, sessionId?: string | null) => Promise<FileChange[]>;
    clearFileChanges: (sessionId: string) => Promise<void>;
    revertFileChange: (id: string) => Promise<void>;
    listPlans: (sessionId?: string | null, workspaceId?: string | null) => Promise<AgentPlanMeta[]>;
    getPlan: (id: string) => Promise<AgentPlanFull | null>;
    archivePlan: (id: string) => Promise<void>;
    cancelPlan: (id: string) => Promise<void>;
    agentResolveApproval: (id: string, approved: boolean, remember?: boolean, sessionId?: string) => Promise<void>;
    /** Answer a pending ask_user question or a present_plan decision. */
    agentAnswerPrompt: (id: string, answer: string) => Promise<void>;
    listPendingApprovals: () => Promise<AgentApproval[]>;
    listAgentConversations: () => Promise<AgentConversationMeta[]>;
    agentAnalytics: () => Promise<AgentAnalytics>;
    appResourceSnapshot: () => Promise<ResourceSnapshot>;
    getAgentConversation: (id: string) => Promise<AgentConversation | null>;
    saveAgentConversation: (args: {
        id: string;
        title?: string | null;
        targets: string[];
        messagesJson: string;
    }) => Promise<AgentConversation>;
    deleteAgentConversation: (id: string) => Promise<void>;
    getAgentDocs: () => Promise<AgentDocs>;
    saveSoul: (content: string) => Promise<void>;
    getHooksConfig: () => Promise<string>;
    saveHooksConfig: (content: string) => Promise<number>;
    reloadHooks: () => Promise<number>;
    hooksStatus: () => Promise<HooksStatus>;
    saveMemoryDoc: (content: string) => Promise<void>;
    saveTasteDoc: (content: string) => Promise<void>;
    listSkills: () => Promise<Skill[]>;
    getSkill: (category: string, name: string) => Promise<string | null>;
    saveSkill: (category: string, name: string, content: string) => Promise<void>;
    deleteSkill: (category: string, name: string) => Promise<void>;
    listCronJobs: () => Promise<CronJob[]>;
    saveCronJob: (input: CronJobInput) => Promise<CronJob>;
    deleteCronJob: (id: string) => Promise<void>;
    runCronJob: (id: string) => Promise<void>;
    startGoal: (text: string) => Promise<string>;
    confirmGoal: (id: string, targets?: string[]) => Promise<void>;
    pauseGoal: (id: string) => Promise<void>;
    continueGoal: (id: string) => Promise<void>;
    stopGoal: (id: string) => Promise<void>;
    getGoal: (id: string) => Promise<GoalSession>;
    listGoals: () => Promise<GoalSession[]>;
    deleteGoal: (id: string) => Promise<void>;
    listInfraProjects: () => Promise<InfraProject[]>;
    saveInfraProject: (input: InfraProjectInput) => Promise<InfraProject>;
    deleteInfraProject: (id: string) => Promise<void>;
    getInfraProject: (id: string) => Promise<InfraProject | null>;
    readProjectFile: (slug: string, path: string) => Promise<string>;
    listCloudAccounts: () => Promise<CloudAccount[]>;
    saveCloudAccount: (input: CloudAccountInput) => Promise<CloudAccount>;
    deleteCloudAccount: (id: string) => Promise<void>;
    listTfcWorkspaces: (accountId: string) => Promise<string[]>;
    listCloudResources: (accountId: string, resource?: string) => Promise<string>;
    startCloudflareOAuthLogin: () => Promise<string>;
    saveCloudflareManualToken: (token: string) => Promise<CloudAccount>;
    listCloudflareZones: (accountId: string) => Promise<CloudflareZone[]>;
    listCloudflareTunnels: (accountId: string) => Promise<CloudflareTunnel[]>;
    createCloudflareTunnel: (accountId: string, name: string) => Promise<CloudflareTunnel>;
    deleteCloudflareTunnel: (accountId: string, tunnelId: string) => Promise<void>;
    getCloudflareTunnelConfig: (accountId: string, tunnelId: string) => Promise<CloudflareTunnelConfig>;
    saveCloudflareTunnelConfig: (accountId: string, tunnelId: string, config: CloudflareTunnelConfig) => Promise<CloudflareTunnelConfig>;
    getCloudflareTunnelToken: (accountId: string, tunnelId: string) => Promise<string>;
    listCloudflareDnsRecords: (accountId: string, zoneId: string) => Promise<CloudflareDnsRecord[]>;
    upsertCloudflareDnsRecord: (accountId: string, zoneId: string, record: CloudflareDnsRecordInput) => Promise<CloudflareDnsRecord>;
    deleteCloudflareDnsRecord: (accountId: string, zoneId: string, recordId: string) => Promise<void>;
    getCloudflareSecuritySettings: (accountId: string, zoneId: string) => Promise<CloudflareSecuritySettings>;
    setCloudflareSecurityLevel: (accountId: string, zoneId: string, level: string) => Promise<string>;
    listCloudflareHistory: (accountId: string) => Promise<CloudflareAuditLog[]>;
    revertCloudflareAction: (accountId: string, logId: string) => Promise<string>;
    listInstalledPlugins: () => Promise<PluginManifest[]>;
    getDisabledPluginIds: () => Promise<string[]>;
    getPluginReadme: (pluginId: string) => Promise<string>;
    installPlugin: (source: string) => Promise<PluginManifest>;
    linkPlugin: (path: string) => Promise<PluginManifest>;
    uninstallPlugin: (pluginId: string) => Promise<void>;
    togglePlugin: (pluginId: string, enabled: boolean) => Promise<boolean>;
    reloadPlugins: () => Promise<PluginManifest[]>;
};
/** Subscribe to streamed output from a CLI provider's login flow. */
export declare function onAiLoginOutput(providerId: string, cb: (ev: StreamEvent) => void): Promise<UnlistenFn>;
/** Subscribe to a chat session's streamed agent output. */
export declare function onAiChatOutput(sessionId: string, cb: (ev: StreamEvent) => void): Promise<UnlistenFn>;
/** Subscribe to pending command-approval requests from the agent. */
export declare function onAgentApproval(cb: (approval: AgentApproval) => void): Promise<UnlistenFn>;
/** Subscribe to clarifying questions the agent asks (ask_user tool). */
export declare function onAgentQuestion(cb: (question: AgentQuestion) => void): Promise<UnlistenFn>;
/** Subscribe to plans the agent presents for approval (present_plan tool). */
export declare function onAgentPlan(cb: (plan: AgentPlan) => void): Promise<UnlistenFn>;
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
export declare function onCanvasCommand(cb: (cmd: CanvasCommand) => void): Promise<UnlistenFn>;
export interface CanvasPreviewPayload {
    id: string;
    title: string;
    html: string;
    width: number;
    height: number;
}
/** Subscribe to live HTML/design sandbox preview requests from the agent. */
export declare function onCanvasPreview(cb: (payload: CanvasPreviewPayload) => void): Promise<UnlistenFn>;
export declare function onVpsUpdated(cb: () => void): Promise<UnlistenFn>;
export declare function onArtifactsChanged(cb: () => void): Promise<UnlistenFn>;
/** Subscribe to live goal-session events (kanban/status/memory updates). */
export declare function onGoalEvent(goalId: string, cb: (ev: StreamEvent) => void): Promise<UnlistenFn>;
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
export type ExternalEditEvent = {
    kind: "opened";
    id: string;
    remote_path: string;
    local_path: string;
} | {
    kind: "saved";
    id: string;
    remote_path: string;
    bytes: number;
} | {
    kind: "skipped";
    id: string;
    remote_path: string;
    reason: string;
} | {
    kind: "failed";
    id: string;
    remote_path: string;
    error: string;
} | {
    kind: "closed";
    id: string;
    remote_path: string;
};
/** Fired when a file open in an external editor is saved back (or refused). */
export declare function onExternalEdit(cb: (e: ExternalEditEvent) => void): Promise<UnlistenFn>;
/** Fired as an SFTP transfer progresses. Each event is a full job snapshot. */
export declare function onTransferProgress(cb: (t: TransferSnapshot) => void): Promise<UnlistenFn>;
/** Fired when the agent edits a file. */
export declare function onFileChange(cb: (c: FileChange) => void): Promise<UnlistenFn>;
/** Fired when an edit is reverted (payload is the change id). */
export declare function onFileChangeReverted(cb: (id: string) => void): Promise<UnlistenFn>;
/** Per-workspace agent status (working / planning / testing / idle). */
export interface AgentWorkspaceStatus {
    workspace_id: string;
    status: string;
}
export declare function onAgentWorkspaceStatus(cb: (s: AgentWorkspaceStatus) => void): Promise<UnlistenFn>;
/** Subscribe to model-download progress. */
export declare function onModelDownload(cb: (p: DownloadProgress) => void): Promise<UnlistenFn>;
/** Subscribe to a session's terminal output (base64-encoded chunks). */
export declare function onSessionOutput(sessionId: string, cb: (bytes: Uint8Array) => void): Promise<UnlistenFn>;
/** Subscribe to a session's connection status changes. */
export declare function onSessionStatus(sessionId: string, cb: (status: SessionStatus) => void): Promise<UnlistenFn>;
export declare function b64ToBytes(b64: string): Uint8Array;
export declare function bytesToB64(data: Uint8Array): string;
export declare function strToB64(s: string): string;
//# sourceMappingURL=tauri.d.ts.map
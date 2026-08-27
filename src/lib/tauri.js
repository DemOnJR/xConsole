import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
/** Human label for a product, for the tree. */
export const DB_PRODUCT_LABEL = {
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
export const api = {
    listVps: () => invoke("list_vps"),
    saveVps: (input) => invoke("save_vps", { input }),
    deleteVps: (id) => invoke("delete_vps", { id }),
    listArtifacts: (query) => invoke("list_artifacts", { query: query ?? null }),
    verifyArtifact: (id) => invoke("verify_artifact", { id }),
    revealArtifact: (id) => invoke("reveal_artifact", { id }),
    deleteArtifact: (id) => invoke("delete_artifact", { id }),
    artifactsDir: () => invoke("artifacts_dir"),
    sshConnect: (vpsId, cols, rows) => invoke("ssh_connect", { vpsId, cols, rows }),
    sshWrite: (sessionId, dataB64) => invoke("ssh_write", { sessionId, dataB64 }),
    sshResize: (sessionId, cols, rows) => invoke("ssh_resize", { sessionId, cols, rows }),
    sshDisconnect: (sessionId) => invoke("ssh_disconnect", { sessionId }),
    sshReplay: (sessionId) => invoke("ssh_replay", { sessionId }),
    /** Git status for a remote path when it is inside a repo; null otherwise. */
    remoteGitBranch: (vpsId, path) => invoke("remote_git_branch", { vpsId, path }),
    localGitBranch: (path) => invoke("local_git_branch", { path }),
    sftpConnect: (vpsId) => invoke("sftp_connect", { vpsId }),
    sftpList: (sessionId, path) => invoke("sftp_list", { sessionId, path }),
    sftpDownload: (sessionId, path) => invoke("sftp_download", { sessionId, path }),
    sftpWrite: (sessionId, path, contentB64) => invoke("sftp_write", { sessionId, path, contentB64 }),
    sftpMkdir: (sessionId, path) => invoke("sftp_mkdir", { sessionId, path }),
    sftpRename: (sessionId, from, to) => invoke("sftp_rename", { sessionId, from, to }),
    sftpRemove: (sessionId, path, isDir) => invoke("sftp_remove", { sessionId, path, isDir }),
    sftpSymlink: (sessionId, linkPath, target) => invoke("sftp_symlink", { sessionId, linkPath, target }),
    sftpDisconnect: (sessionId) => invoke("sftp_disconnect", { sessionId }),
    localFsList: (path) => invoke("local_fs_list", { path: path ?? null }),
    localFsHome: () => invoke("local_fs_home"),
    // --- bulk transfers ---
    pickDirectory: (title) => invoke("pick_directory", { title }),
    pickFiles: (title) => invoke("pick_files", { title }),
    pickFile: (title) => invoke("pick_file", { title }),
    localFsReadText: (path, maxBytes) => invoke("local_fs_read_text", { path, maxBytes: maxBytes ?? null }),
    localFsReadBytes: (path, maxBytes) => invoke("local_fs_read_bytes", { path, maxBytes: maxBytes ?? null }),
    sftpTransferStart: (sessionId, direction, sources, destination, concurrency) => invoke("sftp_transfer_start", {
        sessionId,
        direction,
        sources,
        destination,
        concurrency: concurrency ?? null,
    }),
    sftpArchiveStart: (sessionId, remoteDir, destination, format) => invoke("sftp_archive_start", { sessionId, remoteDir, destination, format }),
    sftpTransferCancel: (id) => invoke("sftp_transfer_cancel", { id }),
    sftpTransferList: () => invoke("sftp_transfer_list"),
    sftpTransferClearFinished: () => invoke("sftp_transfer_clear_finished"),
    // --- database client ---
    dbDiscover: (vpsId) => invoke("db_discover", { vpsId }),
    dbSaveConnection: (endpointId, target) => invoke("db_save_connection", { endpointId, target }),
    dbListConnections: (vpsId) => invoke("db_list_connections", { vpsId }),
    dbForgetConnection: (id) => invoke("db_forget_connection", { id }),
    dbConnectSaved: (id, vpsId) => invoke("db_connect_saved", { id, vpsId }),
    dbConnect: (target) => invoke("db_connect", { target }),
    dbDisconnect: (sessionId) => invoke("db_disconnect", { sessionId }),
    dbUseDatabase: (sessionId, database) => invoke("db_use_database", { sessionId, database }),
    dbListDatabases: (sessionId) => invoke("db_list_databases", { sessionId }),
    dbListTables: (sessionId, schema) => invoke("db_list_tables", { sessionId, schema }),
    dbDescribeTable: (sessionId, schema, table) => invoke("db_describe_table", { sessionId, schema, table }),
    dbSelectPage: (sessionId, schema, table, limit, offset) => invoke("db_select_page", { sessionId, schema, table, limit, offset }),
    dbRunSql: (sessionId, sql) => invoke("db_run_sql", { sessionId, sql }),
    dbUpdateCell: (sessionId, schema, table, column, value, key) => invoke("db_update_cell", {
        sessionId,
        schema,
        table,
        column,
        value,
        key,
    }),
    dbDeleteRow: (sessionId, schema, table, key) => invoke("db_delete_row", { sessionId, schema, table, key }),
    /** Delete several rows in one statement — see the Rust `delete_rows_sql`. */
    dbDeleteRows: (sessionId, schema, table, keys) => invoke("db_delete_rows", { sessionId, schema, table, keys }),
    sftpEditExternal: (sessionId, path) => invoke("sftp_edit_external", { sessionId, path }),
    vpsFileStat: (vpsId, path) => invoke("vps_file_stat", { vpsId, path }),
    vpsFileChmod: (vpsId, path, mode, recursive) => invoke("vps_file_chmod", { vpsId, path, mode, recursive }),
    vpsFileChown: (vpsId, path, owner, group, recursive) => invoke("vps_file_chown", { vpsId, path, owner, group, recursive }),
    vpsFileDelete: (vpsId, path, isDir) => invoke("vps_file_delete", { vpsId, path, isDir }),
    vpsFileRename: (vpsId, from, to) => invoke("vps_file_rename", { vpsId, from, to }),
    vpsFileMkdir: (vpsId, path) => invoke("vps_file_mkdir", { vpsId, path }),
    vpsFileTouch: (vpsId, path) => invoke("vps_file_touch", { vpsId, path }),
    /** Delete a selection, in one remote command. */
    vpsFileDeleteMany: (vpsId, paths) => invoke("vps_file_delete_many", { vpsId, paths }),
    /** Copy (or move) a selection into a directory, in one remote command. */
    vpsFileCopy: (vpsId, sources, destDir, moveThem) => invoke("vps_file_copy", { vpsId, sources, destDir, moveThem }),
    /** Search by name and/or extension, optionally through subdirectories. */
    vpsFileSearch: (vpsId, root, pattern, extensions, recursive) => invoke("vps_file_search", { vpsId, root, pattern, extensions, recursive }),
    /** Create a symlink at `path` pointing at `target`, or repoint an existing one. */
    vpsFileSymlink: (vpsId, path, target) => invoke("vps_file_symlink", { vpsId, path, target }),
    listWorkspaces: () => invoke("list_workspaces"),
    saveWorkspace: (input) => invoke("save_workspace", { input }),
    deleteWorkspace: (id) => invoke("delete_workspace", { id }),
    reorderVps: (ids) => invoke("reorder_vps", { ids }),
    getWorkspaceBrief: (id) => invoke("get_workspace_brief", { id }),
    saveWorkspaceBrief: (id, content) => invoke("save_workspace_brief", { id, content }),
    scanSkillPath: (path) => invoke("scan_skill_path", { path }),
    skillScannerStatus: () => invoke("skill_scanner_status"),
    installSkillScanner: () => invoke("install_skill_scanner"),
    getSystemCapabilities: () => invoke("get_system_capabilities"),
    searchModels: (source, query, baseUrl) => invoke("search_models", { source, query, baseUrl: baseUrl ?? null }),
    hfModelFiles: (repoId) => invoke("hf_model_files", { repoId }),
    downloadModel: (args) => invoke("download_model", {
        source: args.source,
        id: args.id,
        url: args.url ?? null,
        filename: args.filename ?? null,
        baseUrl: args.baseUrl ?? null,
    }),
    listLocalFiles: () => invoke("list_local_files"),
    deleteModel: (source, id, baseUrl) => invoke("delete_model", { source, id, baseUrl: baseUrl ?? null }),
    llamaServerStatus: () => invoke("llama_server_status"),
    llamaServerStart: (modelFile, port, gpuLayers) => invoke("llama_server_start", { modelFile, port, gpuLayers }),
    llamaServerStop: () => invoke("llama_server_stop"),
    ollamaStatus: (baseUrl) => invoke("ollama_status", { baseUrl: baseUrl ?? null }),
    ollamaEnsure: (baseUrl) => invoke("ollama_ensure", { baseUrl: baseUrl ?? null }),
    transcribe: (audioB64, engine, modelFile, lang) => invoke("transcribe", {
        audioB64,
        engine,
        modelFile: modelFile ?? null,
        lang: lang ?? "auto",
    }),
    setupWhisper: () => invoke("setup_whisper"),
    downloadWhisperModel: (modelFile) => invoke("download_whisper_model", { modelFile }),
    synthesize: (text, voice, engine = "piper", instructions) => invoke("synthesize", {
        text,
        voice: voice ?? null,
        engine,
        instructions: instructions ?? null,
    }),
    setupPiper: () => invoke("setup_piper"),
    downloadPiperVoice: (voice) => invoke("download_piper_voice", { voice }),
    setupEdgeTts: () => invoke("setup_edge_tts"),
    setupParakeet: () => invoke("setup_parakeet"),
    setupLlama: () => invoke("setup_llama"),
    listKnownHosts: () => invoke("list_known_hosts"),
    forgetHostKey: (host, port) => invoke("forget_host_key", { host, port }),
    getSetting: (key) => invoke("get_setting", { key }),
    setSetting: (key, value) => invoke("set_setting", { key, value }),
    listSettings: () => invoke("list_settings"),
    deleteSetting: (key) => invoke("delete_setting", { key }),
    listProviders: () => invoke("list_providers"),
    saveProvider: (input) => invoke("save_provider", { input }),
    deleteProvider: (id) => invoke("delete_provider", { id }),
    aiCliLogin: (providerId) => invoke("ai_cli_login", { providerId }),
    aiCliModels: (providerId) => invoke("ai_cli_models", { providerId }),
    /** Autodetect live models for any saved provider using its keychain secret. */
    aiProviderModels: (providerId) => invoke("ai_provider_models", { providerId }),
    /** Probe a cloud provider's /models endpoint (flavor: "openai" | "anthropic"). */
    listModels: (flavor, baseUrl, apiKey) => invoke("ai_list_models", { flavor, baseUrl, apiKey }),
    /** Sync model pricing from live open catalog (e.g. OpenRouter). */
    aiSyncPrices: () => invoke("ai_sync_prices"),
    /** Fetch all known model pricing tables. */
    aiGetModelPrices: () => invoke("ai_get_model_prices"),
    /** Set custom pricing for a model. */
    aiSetModelPrice: (args) => invoke("ai_set_model_price", {
        modelId: args.modelId,
        input: args.input,
        output: args.output,
        cacheRead: args.cacheRead ?? null,
        cacheWrite: args.cacheWrite ?? null,
    }),
    aiChat: (args) => invoke("ai_chat", {
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
    agentCancel: (sessionId) => invoke("agent_cancel", { sessionId }),
    // In-app updater (clone+compile): check GitHub for a newer commit, and (on accept)
    // back up data + re-run the installer's rebuild.
    checkForUpdate: () => invoke("check_for_update"),
    startAppUpdate: () => invoke("start_app_update"),
    getUpdateChannel: () => invoke("get_update_channel"),
    setUpdateChannel: (channel) => invoke("set_update_channel", { channel }),
    // App lock / at-rest DB encryption.
    lockStatus: () => invoke("lock_status"),
    setupLock: (password, remember) => invoke("setup_lock", { password, remember }),
    unlockWithPassword: (password, remember) => invoke("unlock_with_password", { password, remember }),
    changePassword: (oldPassword, newPassword) => invoke("change_password", { oldPassword, newPassword }),
    forgetDevice: () => invoke("forget_device"),
    disableLock: (password) => invoke("disable_lock", { password }),
    /** Encrypt (or decrypt) saved credentials in the OS keychain. Returns how many
     *  were converted. Forward-only for older builds — see the Rust doc comment. */
    setSecretEncryption: (enabled) => invoke("set_secret_encryption", { enabled }),
    /** Requires the master password whenever an app lock is configured. */
    exportUnencryptedBackup: (password) => invoke("export_unencrypted_backup", { password }),
    /** Upload dropped/pasted files to `dir` on a VPS and report where they landed.
     *  `localPaths` are paths the OS gave us; `inline` carries bytes with no path
     *  (a pasted screenshot). */
    terminalUpload: (vpsId, dir, localPaths, inline) => invoke("terminal_upload", { vpsId, dir, localPaths, inline }),
    /** Lock without quitting: closes every shell, re-encrypts the DB, deletes the
     *  plaintext working file, forgets the key. Resolves with how many shells closed. */
    /** Append a line to xconsole.log, for telling apart in-app and external closes. */
    logDiag: (message) => invoke("log_diag", { message }),
    lockNow: () => invoke("lock_now"),
    /** Reset the idle timer. The timeout itself is enforced in the backend. */
    noteActivity: () => invoke("note_activity"),
    getAutoLockMinutes: () => invoke("get_auto_lock_minutes"),
    setAutoLockMinutes: (minutes) => invoke("set_auto_lock_minutes", { minutes }),
    listFileChanges: (sessionId) => invoke("list_file_changes", { sessionId }),
    listFileChangesHistory: (workspaceId, sessionId) => invoke("list_file_changes_history", {
        workspaceId: workspaceId ?? null,
        sessionId: sessionId ?? null,
    }),
    clearFileChanges: (sessionId) => invoke("clear_file_changes", { sessionId }),
    revertFileChange: (id) => invoke("revert_file_change", { id }),
    listPlans: (sessionId, workspaceId) => invoke("list_plans", {
        sessionId: sessionId ?? null,
        workspaceId: workspaceId ?? null,
    }),
    getPlan: (id) => invoke("get_plan", { id }),
    archivePlan: (id) => invoke("archive_plan", { id }),
    cancelPlan: (id) => invoke("cancel_plan", { id }),
    agentResolveApproval: (id, approved, remember, sessionId) => invoke("agent_resolve_approval", {
        id,
        approved,
        remember: remember ?? false,
        sessionId: sessionId ?? null,
    }),
    /** Answer a pending ask_user question or a present_plan decision. */
    agentAnswerPrompt: (id, answer) => invoke("agent_answer_prompt", { id, answer }),
    listPendingApprovals: () => invoke("list_pending_approvals"),
    listAgentConversations: () => invoke("list_agent_conversations"),
    agentAnalytics: () => invoke("agent_analytics"),
    appResourceSnapshot: () => invoke("app_resource_snapshot"),
    getAgentConversation: (id) => invoke("get_agent_conversation", { id }),
    saveAgentConversation: (args) => invoke("save_agent_conversation", {
        input: {
            id: args.id,
            title: args.title ?? null,
            targets: args.targets,
            messages_json: args.messagesJson,
        },
    }),
    deleteAgentConversation: (id) => invoke("delete_agent_conversation", { id }),
    getAgentDocs: () => invoke("get_agent_docs"),
    saveSoul: (content) => invoke("save_soul", { content }),
    getHooksConfig: () => invoke("get_hooks_config"),
    saveHooksConfig: (content) => invoke("save_hooks_config", { content }),
    reloadHooks: () => invoke("reload_hooks"),
    hooksStatus: () => invoke("hooks_status"),
    saveMemoryDoc: (content) => invoke("save_memory_doc", { content }),
    saveTasteDoc: (content) => invoke("save_taste_doc", { content }),
    listSkills: () => invoke("list_skills"),
    getSkill: (category, name) => invoke("get_skill", { category, name }),
    saveSkill: (category, name, content) => invoke("save_skill", { category, name, content }),
    deleteSkill: (category, name) => invoke("delete_skill", { category, name }),
    listCronJobs: () => invoke("list_cron_jobs"),
    saveCronJob: (input) => invoke("save_cron_job", { input }),
    deleteCronJob: (id) => invoke("delete_cron_job", { id }),
    runCronJob: (id) => invoke("run_cron_job", { id }),
    startGoal: (text) => invoke("start_goal", { text }),
    confirmGoal: (id, targets) => invoke("confirm_goal", { id, targets: targets ?? [] }),
    pauseGoal: (id) => invoke("pause_goal", { id }),
    continueGoal: (id) => invoke("continue_goal", { id }),
    stopGoal: (id) => invoke("stop_goal", { id }),
    getGoal: (id) => invoke("get_goal", { id }),
    listGoals: () => invoke("list_goals"),
    deleteGoal: (id) => invoke("delete_goal", { id }),
    listInfraProjects: () => invoke("list_infra_projects"),
    saveInfraProject: (input) => invoke("save_infra_project", { input }),
    deleteInfraProject: (id) => invoke("delete_infra_project", { id }),
    getInfraProject: (id) => invoke("get_infra_project", { id }),
    readProjectFile: (slug, path) => invoke("read_project_file_cmd", { slug, path }),
    listCloudAccounts: () => invoke("list_cloud_accounts"),
    saveCloudAccount: (input) => invoke("save_cloud_account", { input }),
    deleteCloudAccount: (id) => invoke("delete_cloud_account", { id }),
    listTfcWorkspaces: (accountId) => invoke("list_tfc_workspaces", { accountId }),
    listCloudResources: (accountId, resource) => invoke("list_cloud_resources", { accountId, resource }),
    // Cloudflare
    startCloudflareOAuthLogin: () => invoke("start_cloudflare_oauth_login"),
    saveCloudflareManualToken: (token) => invoke("save_cloudflare_manual_token", { token }),
    listCloudflareZones: (accountId) => invoke("list_cloudflare_zones", { accountId }),
    listCloudflareTunnels: (accountId) => invoke("list_cloudflare_tunnels", { accountId }),
    createCloudflareTunnel: (accountId, name) => invoke("create_cloudflare_tunnel", { accountId, name }),
    deleteCloudflareTunnel: (accountId, tunnelId) => invoke("delete_cloudflare_tunnel", { accountId, tunnelId }),
    getCloudflareTunnelConfig: (accountId, tunnelId) => invoke("get_cloudflare_tunnel_config", { accountId, tunnelId }),
    saveCloudflareTunnelConfig: (accountId, tunnelId, config) => invoke("save_cloudflare_tunnel_config", { accountId, tunnelId, config }),
    getCloudflareTunnelToken: (accountId, tunnelId) => invoke("get_cloudflare_tunnel_token", { accountId, tunnelId }),
    listCloudflareDnsRecords: (accountId, zoneId) => invoke("list_cloudflare_dns_records", { accountId, zoneId }),
    upsertCloudflareDnsRecord: (accountId, zoneId, record) => invoke("upsert_cloudflare_dns_record", { accountId, zoneId, record }),
    deleteCloudflareDnsRecord: (accountId, zoneId, recordId) => invoke("delete_cloudflare_dns_record", { accountId, zoneId, recordId }),
    getCloudflareSecuritySettings: (accountId, zoneId) => invoke("get_cloudflare_security_settings", { accountId, zoneId }),
    setCloudflareSecurityLevel: (accountId, zoneId, level) => invoke("set_cloudflare_security_level", { accountId, zoneId, level }),
    listCloudflareHistory: (accountId) => invoke("list_cloudflare_history", { accountId }),
    revertCloudflareAction: (accountId, logId) => invoke("revert_cloudflare_action", { accountId, logId }),
    // Plugin Harness (DeepSeek Harness / Cordis paradigm)
    listInstalledPlugins: () => invoke("list_installed_plugins"),
    getDisabledPluginIds: () => invoke("get_disabled_plugin_ids_cmd"),
    getPluginReadme: (pluginId) => invoke("get_plugin_readme_cmd", { pluginId }),
    installPlugin: (source) => invoke("install_plugin_cmd", { source }),
    linkPlugin: (path) => invoke("link_plugin_cmd", { path }),
    uninstallPlugin: (pluginId) => invoke("uninstall_plugin_cmd", { pluginId }),
    togglePlugin: (pluginId, enabled) => invoke("toggle_plugin_cmd", { pluginId, enabled }),
    reloadPlugins: () => invoke("reload_plugins_cmd"),
};
/** Subscribe to streamed output from a CLI provider's login flow. */
export function onAiLoginOutput(providerId, cb) {
    return listen(`ai://login/${providerId}`, (e) => cb(e.payload));
}
/** Subscribe to a chat session's streamed agent output. */
export function onAiChatOutput(sessionId, cb) {
    return listen(`ai://chat/${sessionId}`, (e) => cb(e.payload));
}
/** Subscribe to pending command-approval requests from the agent. */
export function onAgentApproval(cb) {
    return listen("ai://approval", (e) => cb(e.payload));
}
/** Subscribe to clarifying questions the agent asks (ask_user tool). */
export function onAgentQuestion(cb) {
    return listen("ai://question", (e) => cb(e.payload));
}
/** Subscribe to plans the agent presents for approval (present_plan tool). */
export function onAgentPlan(cb) {
    return listen("ai://plan", (e) => cb(e.payload));
}
/** Subscribe to canvas actions the agent requests (drive the live canvas). */
export function onCanvasCommand(cb) {
    return listen("canvas://command", (e) => cb(e.payload));
}
/** Subscribe to live HTML/design sandbox preview requests from the agent. */
export function onCanvasPreview(cb) {
    return listen("canvas://open-preview", (e) => cb(e.payload));
}
export function onVpsUpdated(cb) {
    return listen("vps://updated", () => cb());
}
export function onArtifactsChanged(cb) {
    return listen("artifacts://changed", () => cb());
}
/** Subscribe to live goal-session events (kanban/status/memory updates). */
export function onGoalEvent(goalId, cb) {
    return listen(`goal://${goalId}`, (e) => cb(e.payload));
}
/** Fired when a file open in an external editor is saved back (or refused). */
export function onExternalEdit(cb) {
    return listen("sftp://external-edit", (e) => cb(e.payload));
}
/** Fired as an SFTP transfer progresses. Each event is a full job snapshot. */
export function onTransferProgress(cb) {
    return listen("sftp://transfer", (e) => cb(e.payload));
}
/** Fired when the agent edits a file. */
export function onFileChange(cb) {
    return listen("agent://file-change", (e) => cb(e.payload));
}
/** Fired when an edit is reverted (payload is the change id). */
export function onFileChangeReverted(cb) {
    return listen("agent://file-change-reverted", (e) => cb(e.payload));
}
export function onAgentWorkspaceStatus(cb) {
    return listen("agent://workspace-status", (e) => cb(e.payload));
}
/** Subscribe to model-download progress. */
export function onModelDownload(cb) {
    return listen("models://download", (e) => cb(e.payload));
}
/** Subscribe to a session's terminal output (base64-encoded chunks). */
export function onSessionOutput(sessionId, cb) {
    return listen(`ssh://${sessionId}/output`, (e) => {
        cb(b64ToBytes(e.payload));
    });
}
/** Subscribe to a session's connection status changes. */
export function onSessionStatus(sessionId, cb) {
    return listen(`ssh://${sessionId}/status`, (e) => cb(e.payload));
}
export function b64ToBytes(b64) {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++)
        out[i] = bin.charCodeAt(i);
    return out;
}
export function bytesToB64(data) {
    let bin = "";
    for (let i = 0; i < data.length; i++)
        bin += String.fromCharCode(data[i]);
    return btoa(bin);
}
export function strToB64(s) {
    return bytesToB64(new TextEncoder().encode(s));
}
//# sourceMappingURL=tauri.js.map
export type ConnState = "connecting" | "connected" | "reconnecting" | "disconnected" | "error";
export interface SessionInfo {
    sessionId?: string;
    status: ConnState;
    hostKey?: string;
    error?: string;
    /** Remote working directory when known (OSC 7 / cd tracking). */
    cwd?: string;
    /** SFTP panels: the remote path currently being browsed. */
    sftpPath?: string;
    /** Git branch when cwd/sftpPath is inside a repo (`null` = not a repo / unknown). */
    gitBranch?: string | null;
    /** Uncommitted changes in that work tree. */
    gitDirty?: boolean;
}
interface SessionState {
    /** Keyed by canvas node id. */
    sessions: Record<string, SessionInfo>;
    setInfo: (nodeId: string, partial: Partial<SessionInfo>) => void;
    remove: (nodeId: string) => void;
}
export declare const useSessionStore: import("zustand").UseBoundStore<import("zustand").StoreApi<SessionState>>;
export {};
//# sourceMappingURL=sessionStore.d.ts.map
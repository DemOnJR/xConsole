import { type ArchiveFormat, type TransferSnapshot } from "../lib/tauri";
/** Files moved at once. Beyond ~8 the SSH window, not the channel count, is the limit. */
export declare const CONCURRENCY_CHOICES: readonly [1, 2, 4, 6, 8, 12, 16];
interface TransferState {
    /** Live jobs keyed by id. The backend sends whole snapshots, so this is a mirror. */
    jobs: Record<string, TransferSnapshot>;
    /** Panel visibility. */
    open: boolean;
    /** How many files to move at once (persisted). */
    concurrency: number;
    /** Remembered download folder, so repeat downloads don't re-prompt (persisted). */
    lastDestination: string | null;
    subscribed: boolean;
    setOpen: (open: boolean) => void;
    setConcurrency: (n: number) => void;
    setLastDestination: (path: string | null) => void;
    /** Start listening for progress and pull any jobs already running. */
    subscribe: () => Promise<() => void>;
    ingest: (snapshot: TransferSnapshot) => void;
    refresh: () => Promise<void>;
    download: (sessionId: string, remotePaths: string[]) => Promise<void>;
    downloadArchive: (sessionId: string, remoteDir: string, format: ArchiveFormat) => Promise<void>;
    upload: (sessionId: string, remoteDir: string, localPaths?: string[]) => Promise<void>;
    cancel: (id: string) => Promise<void>;
    clearFinished: () => Promise<void>;
}
export declare const useTransferStore: import("zustand").UseBoundStore<Omit<import("zustand").StoreApi<TransferState>, "setState" | "persist"> & {
    setState(partial: TransferState | Partial<TransferState> | ((state: TransferState) => TransferState | Partial<TransferState>), replace?: false | undefined): unknown;
    setState(state: TransferState | ((state: TransferState) => TransferState), replace: true): unknown;
    persist: {
        setOptions: (options: Partial<import("zustand/middleware").PersistOptions<TransferState, {
            concurrency: number;
            lastDestination: string | null;
        }, unknown>>) => void;
        clearStorage: () => void;
        rehydrate: () => Promise<void> | void;
        hasHydrated: () => boolean;
        onHydrate: (fn: (state: TransferState) => void) => () => void;
        onFinishHydration: (fn: (state: TransferState) => void) => () => void;
        getOptions: () => Partial<import("zustand/middleware").PersistOptions<TransferState, {
            concurrency: number;
            lastDestination: string | null;
        }, unknown>>;
    };
}>;
export {};
//# sourceMappingURL=transferStore.d.ts.map
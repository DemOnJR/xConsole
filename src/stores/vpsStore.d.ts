import { type Vps, type VpsInput } from "../lib/tauri";
interface VpsState {
    vpsList: Vps[];
    loading: boolean;
    load: () => Promise<void>;
    save: (input: VpsInput) => Promise<Vps>;
    remove: (id: string) => Promise<void>;
    /** Move server `srcId` to the position of `targetId` and persist the order. */
    reorder: (srcId: string, targetId: string) => Promise<void>;
}
export declare const useVpsStore: import("zustand").UseBoundStore<import("zustand").StoreApi<VpsState>>;
export {};
//# sourceMappingURL=vpsStore.d.ts.map
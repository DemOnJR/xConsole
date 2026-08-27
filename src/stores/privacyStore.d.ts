interface PrivacyState {
    maskIps: boolean;
    setMaskIps: (mask: boolean) => void;
    toggleMaskIps: () => void;
}
export declare const usePrivacyStore: import("zustand").UseBoundStore<import("zustand").StoreApi<PrivacyState>>;
export {};
//# sourceMappingURL=privacyStore.d.ts.map
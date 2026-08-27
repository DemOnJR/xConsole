import { create } from "zustand";
export const usePrivacyStore = create((set) => ({
    maskIps: typeof localStorage !== "undefined"
        ? localStorage.getItem("xconsole-mask-ips") === "1"
        : false,
    setMaskIps: (maskIps) => {
        if (typeof localStorage !== "undefined") {
            localStorage.setItem("xconsole-mask-ips", maskIps ? "1" : "0");
        }
        set({ maskIps });
    },
    toggleMaskIps: () => set((s) => {
        const next = !s.maskIps;
        if (typeof localStorage !== "undefined") {
            localStorage.setItem("xconsole-mask-ips", next ? "1" : "0");
        }
        return { maskIps: next };
    }),
}));
//# sourceMappingURL=privacyStore.js.map
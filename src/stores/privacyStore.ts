import { create } from "zustand";

interface PrivacyState {
  maskIps: boolean;
  setMaskIps: (mask: boolean) => void;
  toggleMaskIps: () => void;
}

export const usePrivacyStore = create<PrivacyState>((set) => ({
  maskIps:
    typeof localStorage !== "undefined"
      ? localStorage.getItem("xconsole-mask-ips") === "1"
      : false,

  setMaskIps: (maskIps) => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("xconsole-mask-ips", maskIps ? "1" : "0");
    }
    set({ maskIps });
  },

  toggleMaskIps: () =>
    set((s) => {
      const next = !s.maskIps;
      if (typeof localStorage !== "undefined") {
        localStorage.setItem("xconsole-mask-ips", next ? "1" : "0");
      }
      return { maskIps: next };
    }),
}));

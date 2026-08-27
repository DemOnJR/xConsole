import { create } from "zustand";
import type { Vps } from "../lib/tauri";

interface QuickOpenState {
  isOpen: boolean;
  query: string;
  targetServer: Vps | null;
  open: (opts?: { query?: string; targetServer?: Vps | null }) => void;
  close: () => void;
  toggle: () => void;
  setQuery: (query: string) => void;
}

export const useQuickOpenStore = create<QuickOpenState>((set) => ({
  isOpen: false,
  query: "",
  targetServer: null,

  open: (opts) =>
    set({
      isOpen: true,
      query: opts?.query ?? "",
      targetServer: opts?.targetServer ?? null,
    }),

  close: () =>
    set({
      isOpen: false,
      query: "",
      targetServer: null,
    }),

  toggle: () =>
    set((state) => ({
      isOpen: !state.isOpen,
      query: !state.isOpen ? state.query : "",
      targetServer: null,
    })),

  setQuery: (query: string) => set({ query }),
}));

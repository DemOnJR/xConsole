import { create } from "zustand";

// In-app styled replacement for window.confirm / window.prompt. Components call
// `dialog.confirm(...)` / `dialog.prompt(...)` (promise-based); <DialogHost/>
// renders the active request and resolves the promise on the user's choice.

interface ConfirmOpts {
  title: string;
  message?: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
}

interface PromptOpts {
  title: string;
  label?: string;
  placeholder?: string;
  defaultValue?: string;
  confirmText?: string;
}

export interface ActiveDialog {
  kind: "confirm" | "prompt";
  title: string;
  message?: string;
  label?: string;
  placeholder?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
  danger?: boolean;
}

interface DialogState {
  active: ActiveDialog | null;
  resolver: ((value: boolean | string | null) => void) | null;
  confirm: (opts: ConfirmOpts) => Promise<boolean>;
  prompt: (opts: PromptOpts) => Promise<string | null>;
  /** Resolve the active dialog and clear it. */
  settle: (value: boolean | string | null) => void;
}

export const useDialogStore = create<DialogState>((set, get) => ({
  active: null,
  resolver: null,

  confirm: (opts) =>
    new Promise<boolean>((resolve) => {
      set({
        active: { kind: "confirm", ...opts },
        resolver: (v) => resolve(Boolean(v)),
      });
    }),

  prompt: (opts) =>
    new Promise<string | null>((resolve) => {
      set({
        active: { kind: "prompt", ...opts },
        resolver: (v) => resolve(v === false ? null : (v as string | null)),
      });
    }),

  settle: (value) => {
    const { resolver } = get();
    resolver?.(value);
    set({ active: null, resolver: null });
  },
}));

/** Imperative helpers for use outside React render (event handlers, stores). */
export const dialog = {
  confirm: (opts: ConfirmOpts) => useDialogStore.getState().confirm(opts),
  prompt: (opts: PromptOpts) => useDialogStore.getState().prompt(opts),
};

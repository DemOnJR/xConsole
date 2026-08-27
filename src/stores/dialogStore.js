import { create } from "zustand";
export const useDialogStore = create((set, get) => ({
    active: null,
    resolver: null,
    confirm: (opts) => new Promise((resolve) => {
        set({
            active: { kind: "confirm", ...opts },
            resolver: (v) => resolve(Boolean(v)),
        });
    }),
    prompt: (opts) => new Promise((resolve) => {
        set({
            active: { kind: "prompt", ...opts },
            resolver: (v) => resolve(v === false ? null : v),
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
    confirm: (opts) => useDialogStore.getState().confirm(opts),
    prompt: (opts) => useDialogStore.getState().prompt(opts),
};
//# sourceMappingURL=dialogStore.js.map
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
export declare const useDialogStore: import("zustand").UseBoundStore<import("zustand").StoreApi<DialogState>>;
/** Imperative helpers for use outside React render (event handlers, stores). */
export declare const dialog: {
    confirm: (opts: ConfirmOpts) => Promise<boolean>;
    prompt: (opts: PromptOpts) => Promise<string | null>;
};
export {};
//# sourceMappingURL=dialogStore.d.ts.map
export declare function useInputHistory(setValue: (v: string) => void): {
    record: (next: string) => void;
    undo: () => boolean;
    redo: () => boolean;
    reset: (v?: string) => void;
};
//# sourceMappingURL=useInputHistory.d.ts.map
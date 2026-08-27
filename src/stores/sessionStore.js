import { create } from "zustand";
export const useSessionStore = create((set) => ({
    sessions: {},
    setInfo: (nodeId, partial) => set((s) => {
        const prev = s.sessions[nodeId] ?? { status: "connecting" };
        return {
            sessions: { ...s.sessions, [nodeId]: { ...prev, ...partial } },
        };
    }),
    remove: (nodeId) => set((s) => {
        const next = { ...s.sessions };
        delete next[nodeId];
        return { sessions: next };
    }),
}));
//# sourceMappingURL=sessionStore.js.map
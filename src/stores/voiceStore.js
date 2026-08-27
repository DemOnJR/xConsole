import { create } from "zustand";
const KEY = "ui.voice";
const DEFAULTS = {
    ttsEnabled: false,
    autoSend: true,
    sttEngine: "local",
    sttModel: "ggml-medium.bin",
    sttLang: "auto",
    ttsEngine: "piper",
    ttsPiperVoice: "en_US-hfc_female-medium",
    ttsEdgeVoice: "en-US-AriaNeural",
    ttsCloudVoice: "sage",
    ttsInstructions: "Warm, calm, and conversational. Speak naturally at a relaxed pace.",
    ttsVoice: "",
    ttsRate: 1,
    conversationProvider: "",
};
function load() {
    try {
        const raw = localStorage.getItem(KEY);
        return raw ? { ...DEFAULTS, ...JSON.parse(raw) } : DEFAULTS;
    }
    catch {
        return DEFAULTS;
    }
}
export const useVoiceStore = create((set, get) => ({
    ...load(),
    recording: false,
    transcribing: false,
    setRecording: (recording) => set({ recording }),
    setTranscribing: (transcribing) => set({ transcribing }),
    update: (p) => {
        set(p);
        const { ttsEnabled, autoSend, sttEngine, sttModel, sttLang, ttsEngine, ttsPiperVoice, ttsEdgeVoice, ttsCloudVoice, ttsInstructions, ttsVoice, ttsRate, conversationProvider, } = get();
        try {
            localStorage.setItem(KEY, JSON.stringify({
                ttsEnabled,
                autoSend,
                sttEngine,
                sttModel,
                sttLang,
                ttsEngine,
                ttsPiperVoice,
                ttsEdgeVoice,
                ttsCloudVoice,
                ttsInstructions,
                ttsVoice,
                ttsRate,
                conversationProvider,
            }));
        }
        catch {
            /* ignore */
        }
    },
}));
//# sourceMappingURL=voiceStore.js.map
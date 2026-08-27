export type SttEngine = "local" | "parakeet" | "cloud" | "groq";
export type TtsEngine = "piper" | "edge" | "cloud" | "os";
interface VoicePrefs {
    ttsEnabled: boolean;
    autoSend: boolean;
    sttEngine: SttEngine;
    sttModel: string;
    sttLang: string;
    ttsEngine: TtsEngine;
    ttsPiperVoice: string;
    ttsEdgeVoice: string;
    ttsCloudVoice: string;
    ttsInstructions: string;
    ttsVoice: string;
    ttsRate: number;
    /** Provider id used for spoken (voice) turns; "" = use the main/active provider. */
    conversationProvider: string;
}
interface VoiceState extends VoicePrefs {
    recording: boolean;
    transcribing: boolean;
    setRecording: (v: boolean) => void;
    setTranscribing: (v: boolean) => void;
    update: (p: Partial<VoicePrefs>) => void;
}
export declare const useVoiceStore: import("zustand").UseBoundStore<import("zustand").StoreApi<VoiceState>>;
export {};
//# sourceMappingURL=voiceStore.d.ts.map
import { create } from "zustand";

// Voice settings + transient mic state for the chat composer. Prefs persist to
// localStorage (frontend-only, like the sidebar-collapse flag).

export type SttEngine = "local" | "parakeet" | "cloud" | "groq";
export type TtsEngine = "piper" | "edge" | "cloud" | "os";

const KEY = "ui.voice";

interface VoicePrefs {
  ttsEnabled: boolean;
  autoSend: boolean;
  sttEngine: SttEngine;
  sttModel: string; // whisper GGML filename (local engine)
  sttLang: string; // ISO code ("ro", "en", …) or "auto"
  ttsEngine: TtsEngine; // "piper" = offline neural, "edge" = free cloud, "cloud" = OpenAI, "os" = system
  ttsPiperVoice: string; // Piper voice key (offline)
  ttsEdgeVoice: string; // Edge TTS voice (free cloud)
  ttsCloudVoice: string; // OpenAI voice name
  ttsInstructions: string; // steer the cloud voice's tone/warmth/pacing
  ttsVoice: string; // OS voice name
  ttsRate: number;
  /** Provider id used for spoken (voice) turns; "" = use the main/active provider. */
  conversationProvider: string;
}

const DEFAULTS: VoicePrefs = {
  ttsEnabled: false,
  autoSend: true,
  sttEngine: "local",
  sttModel: "",
  sttLang: "auto",
  ttsEngine: "piper",
  ttsPiperVoice: "en_US-amy-medium",
  ttsEdgeVoice: "en-US-AriaNeural",
  ttsCloudVoice: "sage",
  ttsInstructions: "Warm, calm, and conversational. Speak naturally at a relaxed pace.",
  ttsVoice: "",
  ttsRate: 1,
  conversationProvider: "",
};

function load(): VoicePrefs {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<VoicePrefs>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

interface VoiceState extends VoicePrefs {
  recording: boolean;
  transcribing: boolean;
  setRecording: (v: boolean) => void;
  setTranscribing: (v: boolean) => void;
  update: (p: Partial<VoicePrefs>) => void;
}

export const useVoiceStore = create<VoiceState>((set, get) => ({
  ...load(),
  recording: false,
  transcribing: false,
  setRecording: (recording) => set({ recording }),
  setTranscribing: (transcribing) => set({ transcribing }),
  update: (p) => {
    set(p);
    const {
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
    } = get();
    try {
      localStorage.setItem(
        KEY,
        JSON.stringify({
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
        }),
      );
    } catch {
      /* ignore */
    }
  },
}));

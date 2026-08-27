export declare function ttsAvailable(): boolean;
export declare function listTtsVoices(): string[];
export declare function speak(text: string, opts?: {
    voice?: string;
    rate?: number;
    onEnd?: () => void;
}): void;
/** A token captured before an async synth; if it no longer matches after the synth
 *  resolves, a stop/barge-in happened and the audio should be discarded. */
export declare function currentSpeechEpoch(): number;
/** Queue one synthesized clip for sequential playback. `onDrain` fires once the whole
 *  queue has finished (or was cleared) — used to clear the UI "speaking" flag. */
export declare function enqueueSpeechBytes(b64Audio: string, mime?: string, onDrain?: () => void): void;
/** Play a single clip now, replacing anything queued/playing (one-shot use: Settings
 *  voice previews, non-streaming replies). */
export declare function speakBytes(b64Audio: string, onEnd?: () => void, mime?: string): {
    stop: () => void;
};
/** Stop playback and empty the queue; bumps the epoch so in-flight synths are dropped. */
export declare function clearSpeechQueue(): void;
export declare function cancelSpeech(): void;
/** Strip markdown so TTS speaks prose, not backticks/asterisks/code blocks/emoji. */
export declare function speakableText(md: string): string;
export interface Recorder {
    /** Stop recording and return the captured audio as base64 WAV. */
    stop: () => Promise<string>;
    /** Abort without producing audio. */
    cancel: () => void;
}
export declare function startRecording(): Promise<Recorder>;
export interface Conversation {
    stop: () => void;
}
/**
 * Continuously listen, auto-segmenting on speech→silence (simple energy VAD),
 * and emit each finished utterance as base64 WAV. `shouldPause()` lets the caller
 * mute capture while the assistant is thinking or speaking (avoids self-trigger).
 */
export declare function startConversation(opts: {
    onUtterance: (wavB64: string) => void;
    shouldPause: () => boolean;
    onSpeechStart?: () => void;
}): Promise<Conversation>;
export declare function isSpeaking(): boolean;
//# sourceMappingURL=voice.d.ts.map
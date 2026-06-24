// Browser-side voice helpers: text-to-speech via the OS Web Speech API (offline,
// zero-dependency) and microphone capture encoded to 16 kHz mono WAV (the format
// both whisper.cpp and cloud STT accept).

// ---- Text to speech (the agent speaks back) -------------------------------

export function ttsAvailable(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window;
}

export function listTtsVoices(): string[] {
  if (!ttsAvailable()) return [];
  return window.speechSynthesis.getVoices().map((v) => v.name);
}

export function speak(
  text: string,
  opts?: { voice?: string; rate?: number; onEnd?: () => void },
): void {
  if (!ttsAvailable() || !text.trim()) {
    opts?.onEnd?.();
    return;
  }
  const u = new SpeechSynthesisUtterance(text);
  if (opts?.rate) u.rate = opts.rate;
  if (opts?.voice) {
    const v = window.speechSynthesis.getVoices().find((x) => x.name === opts.voice);
    if (v) u.voice = v;
  }
  if (opts?.onEnd) {
    u.onend = opts.onEnd;
    u.onerror = opts.onEnd;
  }
  window.speechSynthesis.cancel(); // interrupt any current speech
  window.speechSynthesis.speak(u);
}

// Playback for synthesized audio bytes (cloud/local neural TTS). One reused element
// so a new reply / barge-in / mute can cut the current one within a frame.
let activeAudio: HTMLAudioElement | null = null;
let activeOnEnd: (() => void) | null = null;
let cloudSpeaking = false;

export function speakBytes(
  b64Audio: string,
  onEnd?: () => void,
  mime: string = "audio/wav",
): { stop: () => void } {
  stopBytes();
  const bytes = Uint8Array.from(atob(b64Audio), (c) => c.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const a = new Audio(url);
  activeAudio = a;
  activeOnEnd = onEnd ?? null;
  cloudSpeaking = true;
  const done = () => {
    cloudSpeaking = false;
    URL.revokeObjectURL(url);
    if (activeAudio === a) activeAudio = null;
    // Fire the end callback once (covers natural end, error, and stop/cancel).
    const cb = activeOnEnd;
    activeOnEnd = null;
    cb?.();
  };
  a.onended = done;
  a.onerror = done;
  void a.play().catch(done);
  return { stop: () => { try { a.pause(); } catch { /* ignore */ } done(); } };
}

function stopBytes(): void {
  if (activeAudio) {
    try { activeAudio.pause(); } catch { /* ignore */ }
    activeAudio = null;
  }
  cloudSpeaking = false;
  // Notify a waiting onEnd (e.g. so the UI's "speaking" flag clears on cancel).
  const cb = activeOnEnd;
  activeOnEnd = null;
  cb?.();
}

export function cancelSpeech(): void {
  if (ttsAvailable()) window.speechSynthesis.cancel();
  stopBytes();
}

/** Strip markdown so TTS speaks prose, not backticks/asterisks/code blocks/emoji. */
export function speakableText(md: string): string {
  let t = md;
  t = t.replace(/```[\s\S]*?```/g, " "); // fenced code — don't read code aloud
  t = t.replace(/~~~[\s\S]*?~~~/g, " ");
  t = t.replace(/`([^`]+)`/g, "$1"); // inline code → keep text
  t = t.replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1"); // images → alt
  t = t.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1"); // links → text
  t = t.replace(/^\s{0,3}#{1,6}\s+/gm, ""); // headings
  t = t.replace(/^\s{0,3}>\s?/gm, ""); // blockquotes
  t = t.replace(/^\s{0,3}[-*+]\s+/gm, ""); // bullet lists
  t = t.replace(/^\s{0,3}\d+\.\s+/gm, ""); // numbered lists
  t = t.replace(/(\*\*|__)(.*?)\1/g, "$2"); // bold
  t = t.replace(/(\*|_)(.*?)\1/g, "$2"); // italic
  t = t.replace(/~~(.*?)~~/g, "$1"); // strikethrough
  t = t.replace(/\|/g, " "); // table pipes
  t = t.replace(/[#>`*_~]/g, ""); // any stray markdown symbols
  // Emoji / pictographs (read oddly by TTS).
  t = t.replace(
    /[\u{1F000}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}\u{1F1E6}-\u{1F1FF}\u{FE0F}\u{200D}]/gu,
    "",
  );
  return t.replace(/[ \t]{2,}/g, " ").replace(/\n{3,}/g, "\n\n").trim();
}

// ---- Microphone capture → 16 kHz mono WAV (base64) ------------------------

export interface Recorder {
  /** Stop recording and return the captured audio as base64 WAV. */
  stop: () => Promise<string>;
  /** Abort without producing audio. */
  cancel: () => void;
}

export async function startRecording(): Promise<Recorder> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const ctx = new AudioContext();
  const source = ctx.createMediaStreamSource(stream);
  // ScriptProcessor is deprecated but universally available; we never write to
  // its output buffer, so connecting it to the destination stays silent (no echo).
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  processor.onaudioprocess = (e) => {
    chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
  };
  source.connect(processor);
  processor.connect(ctx.destination);

  const teardown = () => {
    processor.disconnect();
    source.disconnect();
    stream.getTracks().forEach((t) => t.stop());
  };

  return {
    stop: async () => {
      const rate = ctx.sampleRate;
      teardown();
      await ctx.close().catch(() => {});
      const pcm = mergeFloat32(chunks);
      const pcm16k = resampleTo16k(pcm, rate);
      return encodeWavBase64(pcm16k);
    },
    cancel: () => {
      teardown();
      void ctx.close().catch(() => {});
    },
  };
}

// ---- Hands-free conversation: continuous listen with silence detection -----

export interface Conversation {
  stop: () => void;
}

/**
 * Continuously listen, auto-segmenting on speech→silence (simple energy VAD),
 * and emit each finished utterance as base64 WAV. `shouldPause()` lets the caller
 * mute capture while the assistant is thinking or speaking (avoids self-trigger).
 */
export async function startConversation(opts: {
  onUtterance: (wavB64: string) => void;
  shouldPause: () => boolean;
  onSpeechStart?: () => void;
}): Promise<Conversation> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const ctx = new AudioContext();
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  const sampleRate = ctx.sampleRate;

  const SILENCE_RMS = 0.012; // below this = silence
  const SILENCE_HOLD_MS = 900; // trailing silence that ends an utterance
  const MIN_SPEECH_MS = 250; // ignore blips
  const MAX_UTTER_MS = 15000; // hard cap

  let speaking = false;
  let frames: Float32Array[] = [];
  let lastVoiceMs = 0;
  let speechStartMs = 0;
  let clockMs = 0; // frame-accumulated clock (Date.now is unavailable in some contexts)

  const flush = () => {
    const captured = frames;
    const dur = clockMs - speechStartMs;
    speaking = false;
    frames = [];
    if (dur >= MIN_SPEECH_MS && captured.length) {
      opts.onUtterance(encodeWavBase64(resampleTo16k(mergeFloat32(captured), sampleRate)));
    }
  };

  processor.onaudioprocess = (e) => {
    const buf = e.inputBuffer.getChannelData(0);
    clockMs += (buf.length / sampleRate) * 1000;
    if (opts.shouldPause()) {
      if (speaking) {
        speaking = false;
        frames = [];
      }
      return;
    }
    let sum = 0;
    for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
    const rms = Math.sqrt(sum / buf.length);
    if (rms >= SILENCE_RMS) {
      if (!speaking) {
        speaking = true;
        speechStartMs = clockMs;
        frames = [];
        opts.onSpeechStart?.();
      }
      frames.push(new Float32Array(buf));
      lastVoiceMs = clockMs;
    } else if (speaking) {
      frames.push(new Float32Array(buf)); // keep trailing silence for natural cut
      if (clockMs - lastVoiceMs >= SILENCE_HOLD_MS) flush();
    }
    if (speaking && clockMs - speechStartMs >= MAX_UTTER_MS) flush();
  };

  source.connect(processor);
  processor.connect(ctx.destination);

  return {
    stop: () => {
      processor.disconnect();
      source.disconnect();
      stream.getTracks().forEach((t) => t.stop());
      void ctx.close().catch(() => {});
    },
  };
}

export function isSpeaking(): boolean {
  return cloudSpeaking || (ttsAvailable() && window.speechSynthesis.speaking);
}

function mergeFloat32(chunks: Float32Array[]): Float32Array {
  let len = 0;
  for (const c of chunks) len += c.length;
  const out = new Float32Array(len);
  let o = 0;
  for (const c of chunks) {
    out.set(c, o);
    o += c.length;
  }
  return out;
}

function resampleTo16k(input: Float32Array, inRate: number): Float32Array {
  if (inRate === 16000) return input;
  const ratio = inRate / 16000;
  const outLen = Math.floor(input.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const idx = i * ratio;
    const i0 = Math.floor(idx);
    const i1 = Math.min(i0 + 1, input.length - 1);
    const frac = idx - i0;
    out[i] = input[i0] * (1 - frac) + input[i1] * frac; // linear interpolation
  }
  return out;
}

/** Encode mono Float32 PCM @16 kHz to a 16-bit WAV, base64 (no data: prefix). */
function encodeWavBase64(pcm: Float32Array): string {
  const numSamples = pcm.length;
  const buffer = new ArrayBuffer(44 + numSamples * 2);
  const view = new DataView(buffer);
  const writeStr = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(off + i, s.charCodeAt(i));
  };
  const sampleRate = 16000;
  writeStr(0, "RIFF");
  view.setUint32(4, 36 + numSamples * 2, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // byte rate
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  writeStr(36, "data");
  view.setUint32(40, numSamples * 2, true);
  let off = 44;
  for (let i = 0; i < numSamples; i++) {
    const s = Math.max(-1, Math.min(1, pcm[i]));
    view.setInt16(off, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    off += 2;
  }
  // ArrayBuffer → base64 in chunks (avoid call-stack limits on large inputs).
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

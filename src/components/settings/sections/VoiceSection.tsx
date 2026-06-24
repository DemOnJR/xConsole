import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useVoiceStore } from "../../../stores/voiceStore";
import { listTtsVoices, ttsAvailable, speak, speakBytes } from "../../../lib/voice";
import { api, onModelDownload } from "../../../lib/tauri";
import { Button, Card, Field, SectionHeader, Select, TextArea, Toggle } from "../ui";

const OPENAI_VOICES = [
  "alloy", "ash", "ballad", "coral", "echo", "fable",
  "nova", "onyx", "sage", "shimmer", "verse", "marin", "cedar",
];

const PIPER_VOICES = [
  { key: "en_US-amy-medium", label: "Amy — English (US), female" },
  { key: "en_US-ryan-high", label: "Ryan — English (US), male" },
  { key: "en_US-hfc_female-medium", label: "HFC — English (US), female" },
  { key: "en_GB-alba-medium", label: "Alba — English (UK), female" },
  { key: "ro_RO-mihai-medium", label: "Mihai — Romanian" },
  { key: "de_DE-thorsten-medium", label: "Thorsten — German" },
  { key: "es_ES-davefx-medium", label: "Davefx — Spanish" },
  { key: "fr_FR-siwis-medium", label: "Siwis — French" },
  { key: "it_IT-riccardo-x_low", label: "Riccardo — Italian" },
];

const EDGE_VOICES = [
  { key: "en-US-AriaNeural", label: "Aria — English (US), female" },
  { key: "en-US-GuyNeural", label: "Guy — English (US), male" },
  { key: "en-GB-SoniaNeural", label: "Sonia — English (UK), female" },
  { key: "ro-RO-AlinaNeural", label: "Alina — Romanian, female" },
  { key: "ro-RO-EmilNeural", label: "Emil — Romanian, male" },
  { key: "de-DE-KatjaNeural", label: "Katja — German" },
  { key: "es-ES-ElviraNeural", label: "Elvira — Spanish" },
  { key: "fr-FR-DeniseNeural", label: "Denise — French" },
  { key: "it-IT-ElsaNeural", label: "Elsa — Italian" },
];

const STT_LANGS = [
  { code: "auto", label: "Auto-detect" },
  { code: "ro", label: "Romanian" },
  { code: "en", label: "English" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "it", label: "Italian" },
  { code: "pt", label: "Portuguese" },
  { code: "ru", label: "Russian" },
  { code: "uk", label: "Ukrainian" },
];

const STT_MODELS = [
  { file: "ggml-base.bin", label: "Base — multilingual, fast (~142 MB)" },
  { file: "ggml-small.bin", label: "Small — multilingual, accurate (~466 MB)" },
  { file: "ggml-medium.bin", label: "Medium — multilingual, best (~1.5 GB)" },
  { file: "ggml-base.en.bin", label: "Base — English only, fastest" },
];

const STT_STORAGE = ["whisper-model", "parakeet-model"];
const TTS_STORAGE = ["piper-voice"];

interface Dl {
  id: string;
  received: number;
  total: number | null;
}

function ProgressBar({ dl }: { dl: Dl | null }) {
  const pct = dl && dl.total ? Math.min(100, Math.round((dl.received / dl.total) * 100)) : null;
  const mb = (n: number) => (n / 1048576).toFixed(0);
  return (
    <div className="mt-2 space-y-1">
      <div className="h-2 w-full overflow-hidden rounded-full bg-[var(--border)]">
        <div
          className={`h-full rounded-full bg-[var(--accent)] transition-all ${pct == null ? "animate-pulse" : ""}`}
          style={{ width: pct == null ? "100%" : `${pct}%` }}
        />
      </div>
      <div className="text-[10px] text-[var(--text-faint)]">
        {dl && pct != null
          ? `${pct}% — ${mb(dl.received)} / ${mb(dl.total ?? 0)} MB`
          : dl
            ? `${mb(dl.received)} MB downloaded…`
            : "Working…"}
      </div>
    </div>
  );
}

function CardTitle({ children, hint }: { children: React.ReactNode; hint?: string }) {
  return (
    <div className="mb-3">
      <div className="text-[11px] font-semibold uppercase tracking-wider text-[var(--text-dim)]">
        {children}
      </div>
      {hint && <p className="mt-0.5 text-[11px] text-[var(--text-faint)]">{hint}</p>}
    </div>
  );
}

export function VoiceSection() {
  const v = useVoiceStore();
  const [osVoices, setOsVoices] = useState<string[]>([]);
  const [providers, setProviders] = useState<{ id: string; name: string; model?: string | null }[]>([]);

  const [sttBusy, setSttBusy] = useState(false);
  const [sttMsg, setSttMsg] = useState("");
  const [ttsBusy, setTtsBusy] = useState(false);
  const [ttsMsg, setTtsMsg] = useState("");
  // Progress keyed by download id, so the whisper-model and piper-voice bars
  // never share/overwrite each other's state.
  const [dls, setDls] = useState<Record<string, Dl>>({});

  useEffect(() => {
    const load = () => setOsVoices(listTtsVoices());
    load();
    if (ttsAvailable()) {
      window.speechSynthesis.onvoiceschanged = load;
      return () => {
        window.speechSynthesis.onvoiceschanged = null;
      };
    }
  }, []);

  useEffect(() => {
    api
      .listProviders()
      .then((ps) => setProviders(ps.filter((p) => p.enabled)))
      .catch(() => setProviders([]));
  }, []);

  // Live download progress for model/voice fetches.
  useEffect(() => {
    let un: UnlistenFn | undefined;
    onModelDownload((p) => {
      if (![...STT_STORAGE, ...TTS_STORAGE].includes(p.id)) return;
      setDls((m) => {
        const next = { ...m };
        if (p.status === "done" || p.status === "error") delete next[p.id];
        else next[p.id] = { id: p.id, received: p.received, total: p.total };
        return next;
      });
    }).then((u) => (un = u));
    return () => un?.();
  }, []);

  // ----- handlers -----
  const setupWhisper = async () => {
    setSttBusy(true);
    setSttMsg("Installing the speech engine + model…");
    try {
      const model = await api.setupWhisper();
      v.update({ sttModel: model });
      setSttMsg(`Ready — model ${model} installed.`);
    } catch (e) {
      setSttMsg(String(e));
    } finally {
      setSttBusy(false);
    }
  };

  const setupParakeet = async () => {
    setSttBusy(true);
    setSttMsg("Installing GPU speech-to-text (Parakeet + model, ~760 MB)…");
    try {
      await api.setupParakeet();
      setSttMsg("GPU speech-to-text ready.");
    } catch (e) {
      setSttMsg(String(e));
    } finally {
      setSttBusy(false);
    }
  };

  const downloadSttModel = async () => {
    const file = v.sttModel || "ggml-small.bin";
    setSttBusy(true);
    setSttMsg(`Downloading ${file}…`);
    try {
      await api.downloadWhisperModel(file);
      v.update({ sttModel: file });
      setSttMsg(`Ready — ${file} installed.`);
    } catch (e) {
      setSttMsg(String(e));
    } finally {
      setSttBusy(false);
    }
  };

  const setupPiper = async () => {
    setTtsBusy(true);
    setTtsMsg("Installing the offline voice engine + voice…");
    try {
      const voice = await api.setupPiper();
      v.update({ ttsPiperVoice: voice });
      setTtsMsg("Offline voice ready.");
    } catch (e) {
      setTtsMsg(String(e));
    } finally {
      setTtsBusy(false);
    }
  };

  const downloadPiperVoice = async () => {
    setTtsBusy(true);
    setTtsMsg(`Downloading voice ${v.ttsPiperVoice}…`);
    try {
      await api.downloadPiperVoice(v.ttsPiperVoice);
      setTtsMsg("Voice ready.");
    } catch (e) {
      setTtsMsg(String(e));
    } finally {
      setTtsBusy(false);
    }
  };

  const setupEdge = async () => {
    setTtsBusy(true);
    setTtsMsg("Setting up the free Edge voice (Python env + edge-tts)…");
    try {
      await api.setupEdgeTts();
      setTtsMsg("Edge voice ready.");
    } catch (e) {
      setTtsMsg(String(e));
    } finally {
      setTtsBusy(false);
    }
  };

  const testVoice = async () => {
    try {
      if (v.ttsEngine === "piper") {
        speakBytes(await api.synthesize("Hello, this is the offline neural voice.", v.ttsPiperVoice, "piper"));
      } else if (v.ttsEngine === "edge") {
        speakBytes(await api.synthesize("Hello, this is the free Edge neural voice.", v.ttsEdgeVoice, "edge"), undefined, "audio/mpeg");
      } else if (v.ttsEngine === "cloud") {
        speakBytes(
          await api.synthesize(
            "Hey, this is how I'll sound. How's the warmth and pace?",
            v.ttsCloudVoice,
            "cloud",
            v.ttsInstructions,
          ),
        );
      } else {
        speak("Voice output is working.", { voice: v.ttsVoice || undefined, rate: v.ttsRate });
      }
    } catch (e) {
      setTtsMsg(String(e));
    }
  };

  const sttDl = dls["whisper-model"] ?? dls["parakeet-model"] ?? null;
  const ttsDl = dls["piper-voice"] ?? null;

  return (
    <div className="space-y-3">
      <SectionHeader
        title="Voice"
        description="Talk to the agent and have it talk back — everything runs locally by default. The mic and 🔊 controls live in the chat composer."
      />

      {/* ── Speech to text ───────────────────────────── */}
      <Card>
        <CardTitle hint="Turns your speech into text for the agent.">
          🎙 Speech to text — your voice → the agent
        </CardTitle>
        <div className="grid grid-cols-2 gap-x-4">
          <Field label="Engine">
            <Select
              value={v.sttEngine}
              onChange={(e) =>
                v.update({ sttEngine: e.target.value as "local" | "parakeet" | "cloud" | "groq" })
              }
            >
              <option value="local">Local — whisper.cpp (offline, CPU)</option>
              <option value="parakeet">Local GPU — Parakeet (AMD/NVIDIA, offline)</option>
              <option value="groq">Groq Whisper (free cloud, needs key)</option>
              <option value="cloud">Cloud — OpenAI (paid, needs key)</option>
            </Select>
          </Field>
          <Field label="Language">
            <Select value={v.sttLang} onChange={(e) => v.update({ sttLang: e.target.value })}>
              {STT_LANGS.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.label}
                </option>
              ))}
            </Select>
          </Field>
        </div>

        {v.sttEngine === "local" && (
          <>
            <Field label="Model" hint="Bigger = more accurate but slower. Medium is best for non-English.">
              <Select
                value={v.sttModel || "ggml-small.bin"}
                onChange={(e) => v.update({ sttModel: e.target.value })}
              >
                {STT_MODELS.map((m) => (
                  <option key={m.file} value={m.file}>
                    {m.label}
                  </option>
                ))}
              </Select>
            </Field>
            <div className="flex flex-wrap items-center gap-2">
              <Button onClick={setupWhisper} disabled={sttBusy}>
                {sttBusy ? "Working…" : "Set up local voice"}
              </Button>
              <Button onClick={downloadSttModel} disabled={sttBusy}>
                Download selected model
              </Button>
            </div>
            {(sttBusy || sttMsg) && <ProgressBar dl={sttDl} />}
            {sttMsg && !sttBusy && (
              <div className="mt-1 text-[11px] text-[var(--text-dim)]">{sttMsg}</div>
            )}
          </>
        )}
        {v.sttEngine === "parakeet" && (
          <div>
            <p className="mb-2 text-[11px] text-[var(--text-faint)]">
              Offline GPU transcription (NVIDIA Parakeet via Vulkan) — runs on your AMD GPU, supports
              Romanian + 24 languages, faster than whisper. ~760 MB one-time install.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <Button onClick={setupParakeet} disabled={sttBusy}>
                {sttBusy ? "Working…" : "Set up GPU speech-to-text"}
              </Button>
            </div>
            {(sttBusy || (sttMsg && sttDl)) && <ProgressBar dl={sttDl} />}
            {sttMsg && !sttBusy && (
              <div className="mt-1 text-[11px] text-[var(--text-dim)]">{sttMsg}</div>
            )}
          </div>
        )}
        {v.sttEngine === "groq" && (
          <p className="text-[11px] text-[var(--text-faint)]">
            Free, fast cloud Whisper. Add a <b>Groq</b> provider (Settings → Providers → Quick add →
            Groq) with your free key and enable it.
          </p>
        )}
        {v.sttEngine === "cloud" && (
          <p className="text-[11px] text-[var(--text-faint)]">
            Uses your OpenAI provider's key (Settings → Providers).
          </p>
        )}
        <label className="mt-2 flex items-center gap-2 text-xs text-[var(--text-dim)]">
          <input
            type="checkbox"
            checked={v.autoSend}
            onChange={(e) => v.update({ autoSend: e.target.checked })}
          />
          Auto-send after I stop talking
        </label>
      </Card>

      {/* ── Text to speech ───────────────────────────── */}
      <Card>
        <CardTitle hint="Reads the agent's replies out loud.">
          🔊 Text to speech — the agent → your speakers
        </CardTitle>

        <div className="mb-3">
          <Toggle
            checked={v.ttsEnabled}
            onChange={(on) => v.update({ ttsEnabled: on })}
            label="Speak the agent's replies aloud"
          />
        </div>

        <div className="grid grid-cols-2 gap-x-4">
          <Field label="Engine">
            <Select
              value={v.ttsEngine}
              onChange={(e) => v.update({ ttsEngine: e.target.value as "piper" | "edge" | "cloud" | "os" })}
            >
              <option value="piper">Offline neural — Piper</option>
              <option value="edge">Edge — free cloud neural</option>
              <option value="os">System voices — offline, robotic</option>
              <option value="cloud">Natural — OpenAI cloud (needs key)</option>
            </Select>
          </Field>

          {v.ttsEngine === "piper" && (
            <Field label="Voice">
              <Select value={v.ttsPiperVoice} onChange={(e) => v.update({ ttsPiperVoice: e.target.value })}>
                {PIPER_VOICES.map((p) => (
                  <option key={p.key} value={p.key}>
                    {p.label}
                  </option>
                ))}
              </Select>
            </Field>
          )}
          {v.ttsEngine === "edge" && (
            <Field label="Voice">
              <Select value={v.ttsEdgeVoice} onChange={(e) => v.update({ ttsEdgeVoice: e.target.value })}>
                {EDGE_VOICES.map((p) => (
                  <option key={p.key} value={p.key}>
                    {p.label}
                  </option>
                ))}
              </Select>
            </Field>
          )}
          {v.ttsEngine === "cloud" && (
            <Field label="Voice">
              <Select value={v.ttsCloudVoice} onChange={(e) => v.update({ ttsCloudVoice: e.target.value })}>
                {OPENAI_VOICES.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>
            </Field>
          )}
          {v.ttsEngine === "os" && (
            <Field label="Voice">
              <Select value={v.ttsVoice} onChange={(e) => v.update({ ttsVoice: e.target.value })}>
                <option value="">System default</option>
                {osVoices.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </Select>
            </Field>
          )}
        </div>

        {v.ttsEngine === "cloud" && (
          <Field label="Voice style — warmth, tone, accent, emotion, pacing">
            <TextArea
              value={v.ttsInstructions}
              onChange={(e) => v.update({ ttsInstructions: e.target.value })}
              rows={2}
              placeholder="Warm, calm, and conversational. Speak naturally at a relaxed pace."
            />
          </Field>
        )}
        {v.ttsEngine === "os" && (
          <Field label={`Rate — ${v.ttsRate.toFixed(1)}×`}>
            <input
              type="range"
              min="0.5"
              max="2"
              step="0.1"
              value={v.ttsRate}
              onChange={(e) => v.update({ ttsRate: Number.parseFloat(e.target.value) })}
              className="w-full"
            />
          </Field>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" onClick={testVoice}>
            ▶ Test voice
          </Button>
          {v.ttsEngine === "piper" && (
            <>
              <Button onClick={setupPiper} disabled={ttsBusy}>
                {ttsBusy ? "Working…" : "Set up offline voice"}
              </Button>
              <Button onClick={downloadPiperVoice} disabled={ttsBusy}>
                Download selected voice
              </Button>
            </>
          )}
          {v.ttsEngine === "edge" && (
            <Button onClick={setupEdge} disabled={ttsBusy}>
              {ttsBusy ? "Working…" : "Set up Edge voice"}
            </Button>
          )}
        </div>
        {v.ttsEngine === "piper" && (ttsBusy || ttsMsg) && <ProgressBar dl={ttsDl} />}
        {(v.ttsEngine === "edge" && ttsBusy) && <ProgressBar dl={null} />}
        {ttsMsg && !ttsBusy && <div className="mt-1 text-[11px] text-[var(--text-dim)]">{ttsMsg}</div>}

        <p className="mt-2 text-[11px] text-[var(--text-faint)]">
          {v.ttsEngine === "piper"
            ? "Runs fully on your machine, no key. Set up once, then pick a voice (incl. Romanian) and download it."
            : v.ttsEngine === "edge"
              ? "Free Microsoft Edge neural voices — no API key, great quality, incl. Romanian. Needs Python (one-time setup) and internet."
              : v.ttsEngine === "cloud"
                ? "Uses your OpenAI provider's key (Settings → Providers)."
                : "System voices are offline but robotic — Piper or Edge sound far better."}
        </p>
      </Card>

      {/* ── Conversation model ───────────────────────── */}
      <Card>
        <CardTitle hint="Optional: let voice chat use a small, fast model while typed/coding turns use your main agent.">
          💬 Conversation model — for spoken turns
        </CardTitle>
        <Field label="Model">
          <Select
            value={v.conversationProvider}
            onChange={(e) => v.update({ conversationProvider: e.target.value })}
          >
            <option value="">Same as main agent</option>
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.model ? ` · ${p.model}` : ""}
              </option>
            ))}
          </Select>
        </Field>
        <p className="text-[11px] text-[var(--text-faint)]">
          Speech-to-text (whisper) and text-to-speech (Piper) are lightweight helpers, not full LLMs —
          only your main agent model is heavy. Leave this on "Same as main agent" to keep it simple.
        </p>
      </Card>
    </div>
  );
}

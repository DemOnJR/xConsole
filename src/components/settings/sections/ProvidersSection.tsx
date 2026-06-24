import { useEffect, useRef, useState } from "react";
import { useSettingsStore } from "../../../stores/settingsStore";
import { dialog } from "../../../stores/dialogStore";
import { api, onAiLoginOutput } from "../../../lib/tauri";
import type { AiProvider, AiProviderInput, ProviderKind } from "../../../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextInput } from "../ui";

const KIND_LABELS: Record<ProviderKind, string> = {
  anthropic: "Anthropic API",
  openai: "Custom (OpenAI-compatible)",
  ollama: "Ollama (local)",
  llamacpp: "llama.cpp (local)",
  cursor: "Cursor (Agent CLI)",
  codex_cli: "Codex CLI",
  opencode_cli: "OpenCode CLI",
};

const OLLAMA_CTX_PRESETS: { value: number; label: string }[] = [
  { value: 2048, label: "2K" },
  { value: 4096, label: "4K" },
  { value: 8192, label: "8K" },
  { value: 16384, label: "16K" },
  { value: 32768, label: "32K" },
  { value: 65536, label: "64K" },
  { value: 131072, label: "128K" },
  { value: 262144, label: "256K" },
];

function ollamaCtxOptions(current: number) {
  if (OLLAMA_CTX_PRESETS.some((o) => o.value === current)) {
    return OLLAMA_CTX_PRESETS;
  }
  return [{ value: current, label: `${current.toLocaleString()} (custom)` }, ...OLLAMA_CTX_PRESETS];
}

const OLLAMA_EXTRA_DEFAULT = {
  num_ctx: 65536,
  num_predict: null as number | null,
  think: false,
  keep_alive: "30m",
};

const KIND_DEFAULTS: Record<ProviderKind, Partial<AiProviderInput>> = {
  anthropic: { model: "claude-sonnet-4-5", base_url: "https://api.anthropic.com" },
  openai: { model: "gpt-4o", base_url: "https://api.openai.com/v1" },
  ollama: {
    model: "qwen3.5:9b",
    base_url: "http://localhost:11434",
    extra_json: JSON.stringify(OLLAMA_EXTRA_DEFAULT),
  },
  llamacpp: { model: "local-model", base_url: "http://127.0.0.1:8080/v1" },
  cursor: { model: "auto", bin_path: "agent" },
  codex_cli: { bin_path: "codex" },
  opencode_cli: { bin_path: "opencode" },
};

// One-click presets for popular providers. Most are OpenAI-compatible, so they
// use the `openai` kind with a base URL; Anthropic uses its own kind. Model ids
// are sensible defaults the user can edit.
const PROVIDER_PRESETS: {
  id: string;
  label: string;
  kind: ProviderKind;
  base_url: string;
  model: string;
}[] = [
  { id: "openai", label: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1", model: "gpt-4o" },
  { id: "anthropic", label: "Anthropic (Claude)", kind: "anthropic", base_url: "https://api.anthropic.com", model: "claude-sonnet-4-5" },
  { id: "openrouter", label: "OpenRouter", kind: "openai", base_url: "https://openrouter.ai/api/v1", model: "openai/gpt-4o" },
  { id: "xai", label: "xAI (Grok)", kind: "openai", base_url: "https://api.x.ai/v1", model: "grok-4" },
  { id: "groq", label: "Groq", kind: "openai", base_url: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile" },
  { id: "together", label: "Together AI", kind: "openai", base_url: "https://api.together.xyz/v1", model: "meta-llama/Llama-3.3-70B-Instruct-Turbo" },
  { id: "deepseek", label: "DeepSeek", kind: "openai", base_url: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { id: "mistral", label: "Mistral", kind: "openai", base_url: "https://api.mistral.ai/v1", model: "mistral-large-latest" },
  { id: "fireworks", label: "Fireworks AI", kind: "openai", base_url: "https://api.fireworks.ai/inference/v1", model: "accounts/fireworks/models/llama-v3p3-70b-instruct" },
  { id: "perplexity", label: "Perplexity", kind: "openai", base_url: "https://api.perplexity.ai", model: "sonar" },
  { id: "gemini", label: "Google Gemini", kind: "openai", base_url: "https://generativelanguage.googleapis.com/v1beta/openai/", model: "gemini-2.5-flash" },
];

const isHttpApi = (kind: ProviderKind) =>
  kind === "anthropic" || kind === "openai" || kind === "llamacpp";

const isOllama = (kind: ProviderKind) => kind === "ollama";

const isCli = (kind: ProviderKind) =>
  kind === "codex_cli" || kind === "opencode_cli" || kind === "cursor";

function parseOllamaExtra(raw?: string | null) {
  if (!raw?.trim()) return { ...OLLAMA_EXTRA_DEFAULT };
  try {
    const v = JSON.parse(raw) as Record<string, unknown>;
    return {
      num_ctx: typeof v.num_ctx === "number" ? v.num_ctx : OLLAMA_EXTRA_DEFAULT.num_ctx,
      num_predict:
        typeof v.num_predict === "number"
          ? v.num_predict
          : v.num_predict === null
            ? null
            : OLLAMA_EXTRA_DEFAULT.num_predict,
      think: typeof v.think === "boolean" ? v.think : OLLAMA_EXTRA_DEFAULT.think,
      keep_alive:
        typeof v.keep_alive === "string" && v.keep_alive
          ? v.keep_alive
          : OLLAMA_EXTRA_DEFAULT.keep_alive,
    };
  } catch {
    return { ...OLLAMA_EXTRA_DEFAULT };
  }
}

function serializeOllamaExtra(extra: ReturnType<typeof parseOllamaExtra>) {
  return JSON.stringify({
    num_ctx: extra.num_ctx,
    num_predict: extra.num_predict,
    think: extra.think,
    keep_alive: extra.keep_alive,
  });
}

function emptyInput(): AiProviderInput {
  return {
    name: "",
    kind: "anthropic",
    enabled: true,
    ...KIND_DEFAULTS.anthropic,
  };
}

/** Free-text model input with a searchable dropdown of locally-available models. */
function ModelCombo({
  value,
  onChange,
  options,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const q = value.trim().toLowerCase();
  const matches = options.filter((o) => !q || o.toLowerCase().includes(q)).slice(0, 10);
  return (
    <div className="relative">
      <input
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        placeholder={placeholder}
        className="w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-sm text-[var(--text)] outline-none focus:border-[var(--accent)]"
      />
      {open && matches.length > 0 && (
        <div className="absolute left-0 right-0 z-10 mt-1 max-h-44 overflow-auto rounded-md border border-[var(--border)] bg-[var(--surface)] shadow-xl">
          {matches.map((o) => (
            <button
              key={o}
              type="button"
              onMouseDown={(e) => {
                e.preventDefault();
                onChange(o);
                setOpen(false);
              }}
              className="block w-full truncate px-2.5 py-1.5 text-left text-xs text-[var(--text-dim)] hover:bg-[var(--border)] hover:text-[var(--text)]"
            >
              {o}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function ProviderForm({
  initial,
  onClose,
}: {
  initial: AiProvider | null;
  onClose: () => void;
}) {
  const saveProvider = useSettingsStore((s) => s.saveProvider);
  const [form, setForm] = useState<AiProviderInput>(
    initial
      ? {
          id: initial.id,
          name: initial.name,
          kind: initial.kind,
          model: initial.model ?? "",
          base_url: initial.base_url ?? "",
          bin_path: initial.bin_path ?? "",
          extra_json: initial.extra_json ?? "",
          enabled: initial.enabled,
          secret: "",
        }
      : emptyInput(),
  );
  const [saving, setSaving] = useState(false);
  const [ollamaExtra, setOllamaExtra] = useState(() =>
    parseOllamaExtra(initial?.extra_json),
  );
  // Models already present on this machine, for the searchable model picker.
  const [localModels, setLocalModels] = useState<string[]>([]);

  // llama.cpp acceleration (CPU / GPU build) — a global setting, installed on demand.
  const [llamaBuild, setLlamaBuild] = useState("cpu");
  const [llamaBusy, setLlamaBusy] = useState(false);
  const [llamaMsg, setLlamaMsg] = useState("");
  useEffect(() => {
    if (form.kind === "llamacpp") {
      api.getSetting("llamacpp.build").then((b) => setLlamaBuild(b || "cpu")).catch(() => {});
    }
  }, [form.kind]);
  const installLlama = async () => {
    setLlamaBusy(true);
    setLlamaMsg("Downloading llama.cpp engine… (one time)");
    try {
      await api.setSetting("llamacpp.build", llamaBuild);
      await api.setupLlama();
      setLlamaMsg(
        llamaBuild === "cpu"
          ? "Installed (CPU)."
          : "Installed (GPU). Models now offload to your GPU.",
      );
    } catch (e) {
      setLlamaMsg(String(e));
    } finally {
      setLlamaBusy(false);
    }
  };

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        if (form.kind === "ollama") {
          const list = await api.searchModels("ollama", "", form.base_url || undefined);
          if (alive) setLocalModels(list.filter((m) => m.installed).map((m) => m.id));
        } else if (form.kind === "llamacpp") {
          const files = await api.listLocalFiles();
          if (alive) setLocalModels(files.map((f) => f.file));
        } else if (alive) {
          setLocalModels([]);
        }
      } catch {
        if (alive) setLocalModels([]);
      }
    })();
    return () => {
      alive = false;
    };
  }, [form.kind, form.base_url]);

  const patch = (p: Partial<AiProviderInput>) => setForm((f) => ({ ...f, ...p }));

  const changeKind = (kind: ProviderKind) => {
    setForm((f) => ({ ...f, kind, ...KIND_DEFAULTS[kind] }));
    if (kind === "ollama") {
      setOllamaExtra(parseOllamaExtra(KIND_DEFAULTS.ollama.extra_json));
    }
  };

  const applyPreset = (id: string) => {
    const p = PROVIDER_PRESETS.find((x) => x.id === id);
    if (!p) return;
    setForm((f) => ({
      ...f,
      name: f.name.trim() ? f.name : p.label,
      kind: p.kind,
      base_url: p.base_url,
      model: p.model,
    }));
  };

  const submit = async () => {
    if (!form.name.trim()) return;
    setSaving(true);
    try {
      const payload: AiProviderInput = {
        ...form,
        extra_json: isOllama(form.kind) ? serializeOllamaExtra(ollamaExtra) : form.extra_json,
      };
      await saveProvider(payload);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const cli = isCli(form.kind);
  const http = isHttpApi(form.kind);
  const ollama = isOllama(form.kind);
  const cursor = form.kind === "cursor";

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="w-[min(520px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit provider" : "Add provider"}
        </h3>

        {!initial && (
          <Field label="Quick add" hint="Prefill a popular provider, then just paste your API key.">
            <Select defaultValue="" onChange={(e) => applyPreset(e.target.value)}>
              <option value="">Choose a provider…</option>
              {PROVIDER_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </Select>
          </Field>
        )}

        <Field label="Type">
          <Select
            value={form.kind}
            onChange={(e) => changeKind(e.target.value as ProviderKind)}
          >
            {(Object.keys(KIND_LABELS) as ProviderKind[]).map((k) => (
              <option key={k} value={k}>
                {KIND_LABELS[k]}
              </option>
            ))}
          </Select>
        </Field>

        <Field label="Name">
          <TextInput
            value={form.name}
            onChange={(e) => patch({ name: e.target.value })}
            placeholder="e.g. Claude (work)"
            autoFocus
          />
        </Field>

        {http && (
          <>
            <Field
              label="Model"
              hint={
                form.kind === "llamacpp" && localModels.length > 0
                  ? "Downloaded GGUF files on this machine — or type the model id your server reports."
                  : undefined
              }
            >
              {form.kind === "llamacpp" ? (
                <ModelCombo
                  value={form.model ?? ""}
                  onChange={(v) => patch({ model: v })}
                  options={localModels}
                  placeholder="model id"
                />
              ) : (
                <TextInput
                  value={form.model ?? ""}
                  onChange={(e) => patch({ model: e.target.value })}
                  placeholder="model id"
                />
              )}
            </Field>
            <Field
              label="Base URL"
              hint={
                form.kind === "llamacpp"
                  ? "Point at a running llama-server. Start one first: llama-server -m <model.gguf> --port 8080 (default URL http://127.0.0.1:8080/v1)."
                  : "Override for self-hosted or proxy endpoints."
              }
            >
              <TextInput
                value={form.base_url ?? ""}
                onChange={(e) => patch({ base_url: e.target.value })}
                placeholder="https://..."
              />
            </Field>
            {form.kind === "llamacpp" && (
              <Field
                label="Acceleration"
                hint="GPU runs local models on your AMD/NVIDIA card (much faster). Pick one, then install the engine."
              >
                <div className="flex flex-wrap items-center gap-2">
                  <Select value={llamaBuild} onChange={(e) => setLlamaBuild(e.target.value)}>
                    <option value="cpu">CPU</option>
                    <option value="vulkan">GPU — Vulkan (AMD / Intel / NVIDIA)</option>
                    <option value="hip">GPU — ROCm (AMD)</option>
                  </Select>
                  <Button onClick={installLlama} disabled={llamaBusy}>
                    {llamaBusy ? "Installing…" : "Install engine"}
                  </Button>
                  {llamaMsg && <span className="text-[11px] text-gray-500">{llamaMsg}</span>}
                </div>
              </Field>
            )}
            {form.kind !== "llamacpp" && (
              <Field
                label={initial?.has_secret ? "API key (leave blank to keep)" : "API key"}
                hint="Stored only in your OS keychain, never in the database."
              >
                <TextInput
                  type="password"
                  value={form.secret ?? ""}
                  onChange={(e) => patch({ secret: e.target.value })}
                  placeholder={initial?.has_secret ? "••••••••" : "sk-..."}
                />
              </Field>
            )}
          </>
        )}

        {ollama && (
          <>
            <Field
              label="Model"
              hint={
                localModels.length > 0
                  ? "Pick one you've installed, or type any tag."
                  : "Must match `ollama list` (e.g. qwen3.5:9b)."
              }
            >
              <ModelCombo
                value={form.model ?? ""}
                onChange={(v) => patch({ model: v })}
                options={localModels}
                placeholder="qwen3.5:9b"
              />
            </Field>
            <Field label="Ollama URL" hint="Default: http://localhost:11434">
              <TextInput
                value={form.base_url ?? ""}
                onChange={(e) => patch({ base_url: e.target.value })}
                placeholder="http://localhost:11434"
              />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Context" hint="Context window (num_ctx). Use 64K+ for VPS agent with snapshots and tools; under 64K uses a compact prompt.">
                <Select
                  value={String(ollamaExtra.num_ctx)}
                  onChange={(e) =>
                    setOllamaExtra((x) => ({
                      ...x,
                      num_ctx: Number.parseInt(e.target.value, 10) || OLLAMA_EXTRA_DEFAULT.num_ctx,
                    }))
                  }
                >
                  {ollamaCtxOptions(ollamaExtra.num_ctx).map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="Max tokens (num_predict)" hint="Leave empty for model default.">
                <TextInput
                  type="number"
                  value={ollamaExtra.num_predict ?? ""}
                  onChange={(e) =>
                    setOllamaExtra((x) => ({
                      ...x,
                      num_predict: e.target.value ? Number.parseInt(e.target.value, 10) : null,
                    }))
                  }
                  placeholder="optional"
                />
              </Field>
            </div>
            <Field label="Keep alive" hint="How long Ollama keeps the model loaded in RAM.">
              <TextInput
                value={ollamaExtra.keep_alive}
                onChange={(e) =>
                  setOllamaExtra((x) => ({ ...x, keep_alive: e.target.value || "30m" }))
                }
                placeholder="30m"
              />
            </Field>
            <label className="mb-3 flex cursor-pointer items-center gap-2 text-xs text-gray-400">
              <input
                type="checkbox"
                checked={ollamaExtra.think}
                onChange={(e) =>
                  setOllamaExtra((x) => ({ ...x, think: e.target.checked }))
                }
                className="rounded border-[#334155]"
              />
              Enable reasoning pass (think) — slower; off is recommended for qwen3.
            </label>
          </>
        )}

        {cursor && (
          <>
            <Field
              label="Binary path"
              hint="The Cursor Agent CLI (`agent`). Install from cursor.com/docs/cli if needed."
            >
              <TextInput
                value={form.bin_path ?? ""}
                onChange={(e) => patch({ bin_path: e.target.value })}
                placeholder="agent"
              />
            </Field>
            <Field
              label="Model"
              hint="Use auto for Cursor's default, or run `agent models` to list IDs."
            >
              <TextInput
                value={form.model ?? ""}
                onChange={(e) => patch({ model: e.target.value })}
                placeholder="auto"
              />
            </Field>
            <Field
              label={initial?.has_secret ? "API key (leave blank to keep)" : "API key"}
              hint="User API key from Cursor Dashboard → Integrations. Or use Login below instead."
            >
              <TextInput
                type="password"
                value={form.secret ?? ""}
                onChange={(e) => patch({ secret: e.target.value })}
                placeholder={initial?.has_secret ? "••••••••" : "key_..."}
              />
            </Field>
          </>
        )}

        {cli && !cursor && (
          <>
            <Field
              label="Binary path"
              hint="Path to the CLI (or just its name if on PATH). Authenticate with Login from the provider list."
            >
              <TextInput
                value={form.bin_path ?? ""}
                onChange={(e) => patch({ bin_path: e.target.value })}
                placeholder={form.kind === "codex_cli" ? "codex" : "opencode"}
              />
            </Field>
            <Field label="Model (optional)">
              <TextInput
                value={form.model ?? ""}
                onChange={(e) => patch({ model: e.target.value })}
                placeholder="default"
              />
            </Field>
          </>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={saving || !form.name.trim()}>
            {saving ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

function CliLoginModal({
  provider,
  onClose,
}: {
  provider: AiProvider;
  onClose: () => void;
}) {
  const [output, setOutput] = useState("");
  const [running, setRunning] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    (async () => {
      unlisten = await onAiLoginOutput(provider.id, (ev) => {
        if (ev.kind === "Text" || ev.kind === "Status") {
          setOutput((o) => o + (ev.kind === "Status" ? `\n${ev.data}\n` : ev.data));
        }
      });
      try {
        await api.aiCliLogin(provider.id);
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setRunning(false);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [provider.id]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView();
  }, [output]);

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="w-[min(640px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
        <h3 className="mb-2 text-sm font-semibold text-gray-100">
          Login — {provider.name}
        </h3>
        <p className="mb-3 text-xs text-gray-500">
          Follow any URL or prompts printed below to authenticate the CLI to your
          account.
        </p>
        <pre className="h-64 overflow-auto whitespace-pre-wrap rounded-md border border-[var(--border)] bg-[var(--bg)] p-3 font-mono text-[11px] leading-relaxed text-gray-300">
          {output || "Starting login..."}
          <div ref={bottomRef} />
        </pre>
        {error && <p className="mt-2 text-xs text-red-400">{error}</p>}
        <div className="mt-4 flex justify-end">
          <Button variant="primary" onClick={onClose}>
            {running ? "Close (keeps running)" : "Done"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProvidersSection() {
  const providers = useSettingsStore((s) => s.providers);
  const removeProvider = useSettingsStore((s) => s.removeProvider);
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<AiProvider | null>(null);
  const [loginFor, setLoginFor] = useState<AiProvider | null>(null);

  return (
    <div>
      <SectionHeader
        title="Providers"
        description="Connect AI backends: direct APIs (Anthropic, Cursor, any OpenAI-compatible endpoint) or local CLIs (Codex, OpenCode) signed in to your own account."
        action={
          <Button
            variant="primary"
            onClick={() => {
              setEditing(null);
              setShowForm(true);
            }}
          >
            <PlusIcon size={13} /> Add
          </Button>
        }
      />

      {providers.length === 0 && (
        <Card className="text-center text-xs text-gray-500">
          No providers yet. Add one to power the agent.
        </Card>
      )}

      <div className="space-y-2">
        {providers.map((p) => (
          <Card key={p.id} className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-sm text-gray-200">{p.name}</span>
                <span className="rounded bg-[var(--border)] px-1.5 py-0.5 text-[10px] text-gray-400">
                  {KIND_LABELS[p.kind]}
                </span>
                {!p.enabled && (
                  <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-300">
                    disabled
                  </span>
                )}
              </div>
              <div className="mt-0.5 truncate text-xs text-gray-500">
                {isCli(p.kind)
                  ? `${p.bin_path || "agent"} · ${p.model || "default"}${p.has_secret ? " · key set" : ""}`
                  : `${p.model || "no model"}${p.has_secret ? " · key set" : " · no key"}`}
              </div>
            </div>
            {isCli(p.kind) && (
              <Button onClick={() => setLoginFor(p)} title="Authenticate this CLI">
                Login
              </Button>
            )}
            <Button
              onClick={() => {
                setEditing(p);
                setShowForm(true);
              }}
            >
              Edit
            </Button>
            <Button
              variant="danger"
              onClick={async () => {
                if (
                  await dialog.confirm({
                    title: "Delete provider",
                    message: `Delete provider "${p.name}"?`,
                    danger: true,
                    confirmText: "Delete",
                  })
                )
                  removeProvider(p.id);
              }}
              title="Delete"
            >
              <TrashIcon size={14} />
            </Button>
          </Card>
        ))}
      </div>

      {showForm && (
        <ProviderForm initial={editing} onClose={() => setShowForm(false)} />
      )}
      {loginFor && (
        <CliLoginModal provider={loginFor} onClose={() => setLoginFor(null)} />
      )}
    </div>
  );
}

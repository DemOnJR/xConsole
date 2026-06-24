import { useEffect, useRef, useState } from "react";
import {
  api,
  onModelDownload,
  type DownloadProgress,
  type HfFile,
  type LlamaStatus,
  type LocalFile,
  type ModelEntry,
  type OllamaStatus,
  type SystemCaps,
} from "../../../lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";
import { dialog } from "../../../stores/dialogStore";

type Pref = "auto" | "vram" | "ram";
type Source = "ollama" | "huggingface";

function fmtBytes(n: number | null): string {
  if (n == null) return "?";
  const gb = n / 1024 / 1024 / 1024;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(n / 1024 / 1024))} MB`;
}

function fits(bytes: number | null, caps: SystemCaps | null, pref: Pref): boolean {
  if (bytes == null || !caps) return true; // unknown size → don't hide
  const margin = 0.7;
  const ram = caps.ram_mb * 1024 * 1024 * margin;
  const vram = caps.vram_mb != null ? caps.vram_mb * 1024 * 1024 * margin : null;
  if (pref === "vram") return vram != null ? bytes <= vram : false;
  if (pref === "ram") return bytes <= ram;
  // auto: a capable GPU (≥6 GB) → VRAM budget, else RAM
  if (caps.vram_mb != null && caps.vram_mb >= 6000 && vram != null) return bytes <= vram;
  return bytes <= ram;
}

export function ModelsSection() {
  const [caps, setCaps] = useState<SystemCaps | null>(null);
  const [source, setSource] = useState<Source>("ollama");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ModelEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [pref, setPref] = useState<Pref>("auto");
  const [showAll, setShowAll] = useState(false);
  const [files, setFiles] = useState<Record<string, HfFile[]>>({});
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [pullName, setPullName] = useState("");
  const [localFiles, setLocalFiles] = useState<LocalFile[]>([]);
  const [llama, setLlama] = useState<LlamaStatus | null>(null);
  const [servePort, setServePort] = useState(8080);
  const [gpuOffload, setGpuOffload] = useState(false);
  const [serveMsg, setServeMsg] = useState("");
  const [serveBusy, setServeBusy] = useState<string | null>(null);
  const baseUrl = useRef("http://localhost:11434");

  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [ollamaBusy, setOllamaBusy] = useState(false);

  const loadLocal = () => api.listLocalFiles().then(setLocalFiles).catch(() => {});
  const loadLlama = () => api.llamaServerStatus().then(setLlama).catch(() => {});
  const loadOllama = () =>
    api.ollamaStatus(baseUrl.current).then(setOllama).catch(() => {});

  const startOllama = async () => {
    setOllamaBusy(true);
    try {
      await api.ollamaEnsure(baseUrl.current);
    } catch (e) {
      console.error(e);
    } finally {
      setOllamaBusy(false);
      loadOllama();
    }
  };

  useEffect(() => {
    api.getSystemCapabilities().then(setCaps).catch(() => {});
    loadLocal();
    loadLlama();
    loadOllama();
    let un: (() => void) | undefined;
    onModelDownload((p) => {
      setProgress((m) => ({ ...m, [p.id]: p }));
      if (p.status === "done") loadLocal();
    }).then((u) => (un = u));
    return () => un?.();
  }, []);

  const serve = async (file: string) => {
    setServeMsg("");
    setServeBusy(file);
    try {
      await api.llamaServerStart(file, servePort, gpuOffload ? 99 : 0);
      setServeMsg(`Serving on http://127.0.0.1:${servePort}/v1 — point a llama.cpp provider there.`);
      loadLlama();
    } catch (e) {
      setServeMsg(String(e));
    } finally {
      setServeBusy(null);
    }
  };

  const stopServer = async () => {
    await api.llamaServerStop().catch(() => {});
    setServeMsg("");
    loadLlama();
  };

  const search = async () => {
    setLoading(true);
    try {
      setResults(await api.searchModels(source, query, baseUrl.current));
    } catch (e) {
      setResults([]);
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  // Auto-load the Ollama catalog the first time.
  useEffect(() => {
    void search();
    if (source === "ollama") loadOllama();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source]);

  const toggleFiles = async (repoId: string) => {
    if (files[repoId]) {
      setFiles((f) => {
        const next = { ...f };
        delete next[repoId];
        return next;
      });
      return;
    }
    try {
      setFiles((f) => ({ ...f, [repoId]: [] }));
      const list = await api.hfModelFiles(repoId);
      setFiles((f) => ({ ...f, [repoId]: list }));
    } catch (e) {
      console.error(e);
    }
  };

  const pullOllama = async (id: string) => {
    try {
      await api.ollamaEnsure(baseUrl.current);
      loadOllama();
    } catch {
      /* surfaced below if the pull then fails */
    }
    return api.downloadModel({ source: "ollama", id, baseUrl: baseUrl.current }).catch((e) =>
      setProgress((m) => ({
        ...m,
        [id]: { id, received: 0, total: null, status: "error", message: String(e) },
      })),
    );
  };

  const downloadHf = (repoId: string, f: HfFile) => {
    const id = `${repoId}/${f.file}`;
    api
      .downloadModel({ source: "huggingface", id, url: f.url, filename: f.file })
      .catch((e) =>
        setProgress((m) => ({
          ...m,
          [id]: { id, received: 0, total: null, status: "error", message: String(e) },
        })),
      );
  };

  const removeOllama = async (id: string) => {
    if (
      !(await dialog.confirm({
        title: "Remove model",
        message: `Remove "${id}" from Ollama? This frees the disk space.`,
        danger: true,
        confirmText: "Remove",
      }))
    )
      return;
    try {
      await api.deleteModel("ollama", id, baseUrl.current);
      await search();
    } catch (e) {
      console.error(e);
    }
  };

  const removeLocal = async (file: string) => {
    if (
      !(await dialog.confirm({
        title: "Delete file",
        message: `Delete ${file}?`,
        danger: true,
        confirmText: "Delete",
      }))
    )
      return;
    try {
      await api.deleteModel("gguf", file);
      loadLocal();
    } catch (e) {
      console.error(e);
    }
  };

  const Badge = ({ bytes }: { bytes: number | null }) => {
    const ok = fits(bytes, caps, pref);
    return (
      <span
        className={`rounded px-1.5 py-0.5 text-[10px] ${
          ok ? "bg-green-600/20 text-green-300" : "bg-amber-600/20 text-amber-300"
        }`}
      >
        {fmtBytes(bytes)}
        {bytes != null ? (ok ? " · fits" : " · too big") : ""}
      </span>
    );
  };

  const Progress = ({ id }: { id: string }) => {
    const p = progress[id];
    if (!p) return null;
    if (p.status === "error")
      return <span className="text-[10px] text-red-400">{p.message ?? "failed"}</span>;
    if (p.status === "done")
      return <span className="text-[10px] text-green-400">✓ installed</span>;
    const pct = p.total ? Math.round((p.received / p.total) * 100) : null;
    return (
      <span className="text-[10px] text-[var(--text-dim)]">
        {pct != null ? `${pct}%` : fmtBytes(p.received)}…
      </span>
    );
  };

  const visible = results.filter((m) => showAll || fits(m.size_bytes, caps, pref));

  const inputCls =
    "rounded-md border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1.5 text-sm text-[var(--text)] outline-none focus:border-[var(--accent)]";

  return (
    <div className="space-y-4">
      {/* Detected hardware */}
      <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 text-xs text-[var(--text-dim)]">
        <span className="text-[var(--text)]">This machine:</span>{" "}
        {caps ? (
          <>
            {fmtBytes(caps.ram_mb * 1024 * 1024)} RAM
            {caps.vram_mb != null
              ? ` · ${fmtBytes(caps.vram_mb * 1024 * 1024)} VRAM (${caps.gpu_name ?? "GPU"})`
              : " · no dedicated GPU VRAM detected (using RAM)"}
          </>
        ) : (
          "detecting…"
        )}
      </div>

      {/* Downloaded GGUF files + run them with the managed llama.cpp server */}
      {localFiles.length > 0 && (
        <div className="rounded-lg border border-[var(--border)] bg-[var(--surface)] p-2.5">
          <div className="mb-1.5 flex flex-wrap items-center gap-2">
            <span className="text-[10px] uppercase tracking-wider text-[var(--text-dim)]">
              Downloaded GGUF files
            </span>
            <span className="ml-auto flex items-center gap-2 text-[11px] text-[var(--text-dim)]">
              <span>Serve port</span>
              <input
                type="number"
                value={servePort}
                onChange={(e) => setServePort(Number.parseInt(e.target.value, 10) || 8080)}
                className="w-16 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-[var(--text)] outline-none"
              />
              <label className="flex items-center gap-1">
                <input
                  type="checkbox"
                  checked={gpuOffload}
                  onChange={(e) => setGpuOffload(e.target.checked)}
                />
                GPU
              </label>
            </span>
          </div>

          {llama?.bin == null && (
            <p className="mb-1.5 text-[11px] text-amber-300">
              llama-server not found — install llama.cpp (so `llama-server` is on PATH) to run these in-app.
            </p>
          )}
          {llama?.running && (
            <div className="mb-1.5 flex items-center gap-2 rounded bg-green-600/15 px-2 py-1 text-[11px] text-green-300">
              <span className="min-w-0 flex-1 truncate">
                ● Serving {llama.model?.split(/[/\\]/).pop()} on :{llama.port}
              </span>
              <button
                onClick={stopServer}
                className="rounded border border-[var(--border)] px-2 py-0.5 text-red-300 hover:bg-red-600/20"
              >
                Stop
              </button>
            </div>
          )}
          {serveMsg && <p className="mb-1.5 text-[11px] text-[var(--text-dim)]">{serveMsg}</p>}

          <div className="space-y-1">
            {localFiles.map((f) => (
              <div key={f.file} className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--text-dim)]">
                  {f.file}
                </span>
                <span className="text-[10px] text-[var(--text-faint)]">{fmtBytes(f.size_bytes)}</span>
                <button
                  disabled={!llama?.bin || serveBusy === f.file}
                  onClick={() => serve(f.file)}
                  className="rounded-md bg-[var(--accent)] px-2 py-0.5 text-[11px] text-[var(--accent-fg)] hover:opacity-90 disabled:opacity-40"
                >
                  {serveBusy === f.file ? "Starting…" : "Serve"}
                </button>
                <button
                  onClick={() => removeLocal(f.file)}
                  className="rounded-md border border-[var(--border)] px-2 py-0.5 text-[11px] text-red-300 hover:bg-red-600/20"
                >
                  Remove
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Controls */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex overflow-hidden rounded-md border border-[var(--border)]">
          {(["ollama", "huggingface"] as Source[]).map((s) => (
            <button
              key={s}
              onClick={() => setSource(s)}
              className={`px-3 py-1.5 text-xs ${
                source === s
                  ? "bg-[var(--accent)] text-[var(--accent-fg)]"
                  : "text-[var(--text-dim)] hover:bg-[var(--border)]"
              }`}
            >
              {s === "ollama" ? "Ollama" : "Hugging Face (GGUF)"}
            </button>
          ))}
        </div>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && search()}
          placeholder={source === "ollama" ? "Filter catalog…" : "Search GGUF models…"}
          className={`${inputCls} min-w-[160px] flex-1`}
        />
        <button onClick={search} className={`${inputCls} cursor-pointer hover:bg-[var(--border)]`}>
          Search
        </button>
        <select value={pref} onChange={(e) => setPref(e.target.value as Pref)} className={inputCls}>
          <option value="auto">Fit: Auto</option>
          <option value="vram">Fit: GPU VRAM</option>
          <option value="ram">Fit: RAM</option>
        </select>
        <label className="flex items-center gap-1.5 text-xs text-[var(--text-dim)]">
          <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
          Show all
        </label>
      </div>

      {source === "ollama" && (
        <div className="flex items-center gap-2">
          <input
            value={pullName}
            onChange={(e) => setPullName(e.target.value)}
            placeholder="Pull any Ollama model by name, e.g. llama3.1:8b"
            className={`${inputCls} flex-1`}
          />
          <button
            onClick={() => pullName.trim() && pullOllama(pullName.trim())}
            className={`${inputCls} cursor-pointer hover:bg-[var(--border)]`}
          >
            Pull
          </button>
        </div>
      )}

      {/* Ollama daemon status / start / install */}
      {source === "ollama" && ollama && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-2.5 py-2 text-xs">
          {!ollama.installed ? (
            <>
              <span className="text-amber-300">Ollama isn't installed on this machine.</span>
              <button
                onClick={() => void openUrl("https://ollama.com/download")}
                className="ml-auto rounded-md bg-[var(--accent)] px-2.5 py-1 text-[var(--accent-fg)] hover:opacity-90"
              >
                Download Ollama
              </button>
            </>
          ) : ollama.running ? (
            <span className="text-green-300">● Ollama is running — models start automatically.</span>
          ) : (
            <>
              <span className="text-[var(--text-dim)]">Ollama is installed but not running.</span>
              <button
                onClick={startOllama}
                disabled={ollamaBusy}
                className="ml-auto rounded-md bg-[var(--accent)] px-2.5 py-1 text-[var(--accent-fg)] hover:opacity-90 disabled:opacity-40"
              >
                {ollamaBusy ? "Starting…" : "Start Ollama"}
              </button>
            </>
          )}
        </div>
      )}

      {/* Results */}
      <div className="space-y-1.5">
        {loading && <p className="text-xs text-[var(--text-dim)]">Searching…</p>}
        {!loading && visible.length === 0 && (
          <p className="text-xs text-[var(--text-dim)]">
            No matching models{!showAll ? " fit this machine — try “Show all”." : "."}
          </p>
        )}
        {visible.map((m) => (
          <div
            key={`${m.source}:${m.id}`}
            className="rounded-md border border-[var(--border)] bg-[var(--surface)] p-2.5"
          >
            <div className="flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm text-[var(--text)]">{m.name}</div>
                <div className="truncate text-[11px] text-[var(--text-faint)]">{m.detail}</div>
              </div>
              {m.source === "ollama" && <Badge bytes={m.size_bytes} />}
              {m.source === "ollama" &&
                (m.installed ? (
                  <button
                    onClick={() => removeOllama(m.id)}
                    className="rounded-md border border-[var(--border)] px-2 py-1 text-[11px] text-red-300 hover:bg-red-600/20"
                  >
                    Remove
                  </button>
                ) : (
                  <>
                    <Progress id={m.id} />
                    <button
                      onClick={() => pullOllama(m.id)}
                      className="rounded-md bg-[var(--accent)] px-2.5 py-1 text-[11px] text-[var(--accent-fg)] hover:opacity-90"
                    >
                      Pull
                    </button>
                  </>
                ))}
              {m.source === "huggingface" && (
                <button
                  onClick={() => toggleFiles(m.id)}
                  className="rounded-md border border-[var(--border)] px-2.5 py-1 text-[11px] text-[var(--text-dim)] hover:bg-[var(--border)]"
                >
                  {files[m.id] ? "Hide files" : "Files"}
                </button>
              )}
            </div>

            {m.source === "huggingface" && files[m.id] && (
              <div className="mt-2 space-y-1 border-t border-[var(--border)] pt-2">
                {files[m.id].length === 0 && (
                  <p className="text-[11px] text-[var(--text-faint)]">Loading files…</p>
                )}
                {files[m.id]
                  .filter((f) => showAll || fits(f.size_bytes, caps, pref))
                  .map((f) => {
                    const id = `${m.id}/${f.file}`;
                    return (
                      <div key={f.file} className="flex items-center gap-2">
                        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-[var(--text-dim)]">
                          {f.file}
                        </span>
                        <Badge bytes={f.size_bytes} />
                        <Progress id={id} />
                        <button
                          onClick={() => downloadHf(m.id, f)}
                          className="rounded-md bg-[var(--accent)] px-2.5 py-1 text-[11px] text-[var(--accent-fg)] hover:opacity-90"
                        >
                          Download
                        </button>
                      </div>
                    );
                  })}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

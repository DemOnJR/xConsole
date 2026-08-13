import { useEffect, useMemo, useState } from "react";
import { api, onArtifactsChanged, type Artifact } from "../../../lib/tauri";
import { Button, Card, SectionHeader } from "../ui";

function shortHash(sha: string): string {
  return sha.length > 16 ? `${sha.slice(0, 12)}…` : sha;
}

function kindLabel(kind: string): string {
  if (kind === "ssh_key") return "SSH private key";
  if (kind === "ssh_pub") return "SSH public key";
  if (kind === "download") return "Download";
  return "File";
}

export function ArtifactsSection() {
  const [items, setItems] = useState<Artifact[]>([]);
  const [query, setQuery] = useState("");
  const [dir, setDir] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const load = async (q?: string) => {
    try {
      const list = await api.listArtifacts(q?.trim() || null);
      setItems(list);
    } catch (e) {
      setMsg(String(e));
    }
  };

  useEffect(() => {
    void load();
    void api.artifactsDir().then(setDir).catch(() => {});
    let un: (() => void) | undefined;
    void onArtifactsChanged(() => {
      void load(query);
    }).then((u) => {
      un = u;
    });
    return () => un?.();
  }, []);

  const filtered = useMemo(() => items, [items]);

  const run = async (id: string, fn: () => Promise<void>, ok: string) => {
    setBusy(id);
    setMsg("");
    try {
      await fn();
      setMsg(ok);
      await load(query);
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div>
      <SectionHeader
        title="Artifacts"
        description="Files the agent created on this PC — SSH key backups, downloads, and writes. Hashes are checked on save so a half-written file is rejected."
      />

      {dir && (
        <p className="mb-3 font-mono text-[11px] text-[var(--text-faint)]">Folder: {dir}</p>
      )}

      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void load(query);
        }}
        placeholder="Search name, path, hash…"
        className="mb-3 w-full rounded-md border border-[var(--border)] bg-[var(--bg)] px-3 py-2 text-sm text-gray-100 outline-none focus:border-[var(--accent)]"
      />
      <div className="mb-3 flex gap-2">
        <Button type="button" onClick={() => void load(query)}>
          Search
        </Button>
        <Button type="button" onClick={() => { setQuery(""); void load(); }}>
          All
        </Button>
      </div>

      {msg && <p className="mb-2 text-[11px] text-[var(--text-dim)]">{msg}</p>}

      {filtered.length === 0 ? (
        <Card>
          <p className="text-xs text-[var(--text-dim)]">
            Nothing yet. When the agent saves an SSH key backup or writes a local file, it shows up here with a SHA-256 checksum.
          </p>
        </Card>
      ) : (
        <div className="space-y-2">
          {filtered.map((a) => (
            <Card key={a.id}>
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="truncate text-sm text-gray-200">{a.name}</div>
                  <div className="mt-0.5 text-[10px] uppercase tracking-wide text-[var(--text-faint)]">
                    {kindLabel(a.kind)}
                    {a.secret ? " · secret (contents hidden from the agent)" : ""}
                    {" · "}
                    {a.size} bytes
                  </div>
                  <div className="mt-1 truncate font-mono text-[10px] text-[var(--text-faint)]" title={a.path}>
                    {a.path}
                  </div>
                  <div className="mt-0.5 font-mono text-[10px] text-[var(--text-dim)]" title={a.sha256}>
                    sha256 {shortHash(a.sha256)}
                  </div>
                </div>
                <div className="flex shrink-0 flex-col gap-1">
                  <Button
                    type="button"
                    disabled={busy === a.id}
                    onClick={() => void run(a.id, () => api.revealArtifact(a.id), "Opened in file manager")}
                  >
                    Reveal
                  </Button>
                  <Button
                    type="button"
                    disabled={busy === a.id}
                    onClick={() =>
                      void run(
                        a.id,
                        async () => {
                          const ok = await api.verifyArtifact(a.id);
                          if (!ok) throw new Error("Hash mismatch — file was changed or corrupted");
                        },
                        "Hash matches — file is intact",
                      )
                    }
                  >
                    Verify
                  </Button>
                  {a.kind !== "ssh_key" && a.kind !== "ssh_pub" ? (
                    <Button
                      type="button"
                      disabled={busy === a.id}
                      onClick={() => void run(a.id, () => api.deleteArtifact(a.id), "Removed from the list")}
                    >
                      Remove
                    </Button>
                  ) : null}
                </div>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

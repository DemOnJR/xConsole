import { useEffect, useMemo, useState } from "react";
import { api, type ScannerStatus, type Skill } from "../../../lib/tauri";
import { dialog } from "../../../stores/dialogStore";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, TextArea, TextInput } from "../ui";

/**
 * Skill security scanner: install/status of NVIDIA SkillSpector + an opt-in deep
 * (LLM-backed) analysis that runs against the local model. Every skill — including ones
 * the agent researches itself — is scanned before it's saved or installed.
 */
function SkillScannerCard() {
  const [status, setStatus] = useState<ScannerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [deep, setDeep] = useState(false);
  const [model, setModel] = useState("");

  const refresh = () => api.skillScannerStatus().then(setStatus).catch(() => {});
  useEffect(() => {
    refresh();
    api.getSetting("skills.scanner_deep").then((v) => setDeep(v === "true"));
    api.getSetting("skills.scanner_model").then((v) => setModel(v ?? ""));
  }, []);

  const install = async () => {
    setBusy(true);
    setMsg("Installing SkillSpector (this can take a minute)…");
    try {
      setMsg(await api.installSkillScanner());
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const toggleDeep = async () => {
    const next = !deep;
    setDeep(next);
    await api.setSetting("skills.scanner_deep", next ? "true" : "false");
  };

  const saveModel = async () => {
    await api.setSetting("skills.scanner_model", model.trim());
    setMsg(model.trim() ? `Deep-scan model set to ${model.trim()}.` : "Deep-scan model cleared (uses the active model).");
  };

  const installed = status?.installed ?? false;

  return (
    <Card className="mb-4">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm text-gray-200">Skill security scanner</div>
          <div className="mt-0.5 text-xs text-gray-500">
            Skills — including ones the agent researches with{" "}
            <span className="font-mono">learn_skill</span> — are scanned before they're
            saved or installed. NVIDIA SkillSpector is the strong static analyzer; without
            it a built-in heuristic is the fallback.
          </div>
        </div>
        <div className="shrink-0">
          {installed ? (
            <span className="rounded-full bg-emerald-500/15 px-2 py-1 text-[11px] text-emerald-400">
              SkillSpector active
            </span>
          ) : (
            <span className="rounded-full bg-amber-500/15 px-2 py-1 text-[11px] text-amber-400">
              Built-in heuristic
            </span>
          )}
        </div>
      </div>

      <div className="mt-2 font-mono text-[11px] text-gray-500">
        {installed
          ? status?.version ?? "SkillSpector installed"
          : status?.uv_available
            ? "SkillSpector not installed (uv is available)"
            : "SkillSpector not installed — uv is required to install it"}
      </div>

      {!installed && (
        <div className="mt-3 flex items-center gap-2">
          <Button
            onClick={() => void install()}
            disabled={busy || !(status?.uv_available ?? false)}
            title={status?.uv_available ? "Install SkillSpector via uv" : "Install uv first"}
          >
            {busy ? "Installing…" : "Install SkillSpector"}
          </Button>
          {!status?.uv_available && (
            <span className="text-[11px] text-gray-500">
              Install uv from docs.astral.sh/uv first.
            </span>
          )}
        </div>
      )}

      {/* Deep (LLM-backed) analysis via the local model. */}
      <div className="mt-3 border-t border-[var(--border)] pt-3">
        <label className="flex items-start gap-2.5 text-sm text-gray-200">
          <input
            type="checkbox"
            className="mt-0.5 accent-[var(--accent)]"
            checked={deep}
            onChange={() => void toggleDeep()}
            disabled={!installed}
          />
          <span>
            Deep analysis with the local model
            <span className="ml-2 block text-xs font-normal text-gray-500">
              Adds SkillSpector's LLM-backed semantic checks, run against your local
              Ollama (no API key, nothing leaves your machine). Slower — best for
              installs. Use a non-thinking instruct model (or a cloud model); thinking
              models (qwen3.x) overrun the scanner's token budget and it falls back to the
              fast static scan, which always runs regardless.
              {!installed && " Requires SkillSpector."}
            </span>
          </span>
        </label>

        {deep && installed && (
          <div className="mt-2 flex items-end gap-2">
            <div className="flex-1">
              <Field label="Deep-scan model (optional — defaults to the active model)">
                <TextInput
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  placeholder="qwen3.5:9b"
                />
              </Field>
            </div>
            <Button onClick={() => void saveModel()}>Save</Button>
          </div>
        )}
      </div>

      {msg && <div className="mt-2 text-[11px] text-gray-400">{msg}</div>}
    </Card>
  );
}

const SKILL_TEMPLATE =
  "---\ndescription: One-line summary of what this skill does.\n---\n\n# Skill title\n\nSteps the agent should follow...\n";

function SkillEditor({
  initial,
  onClose,
  onSaved,
}: {
  initial: Skill | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [category, setCategory] = useState(initial?.category ?? "devops");
  const [name, setName] = useState(initial?.name ?? "");
  const [content, setContent] = useState(initial ? "" : SKILL_TEMPLATE);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (initial) {
      api.getSkill(initial.category, initial.name).then((c) => setContent(c ?? ""));
    }
  }, [initial]);

  const submit = async () => {
    if (!category.trim() || !name.trim()) return;
    setSaving(true);
    try {
      // If the name/category changed on an existing skill, remove the old one.
      if (initial && (initial.category !== category || initial.name !== name)) {
        await api.deleteSkill(initial.category, initial.name);
      }
      await api.saveSkill(category, name, content);
      onSaved();
      onClose();
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 p-6"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="w-[min(680px,92vw)] rounded-xl border border-[var(--border)] bg-[var(--surface-2)] p-5 shadow-2xl">
        <h3 className="mb-4 text-sm font-semibold text-gray-100">
          {initial ? "Edit skill" : "New skill"}
        </h3>
        <div className="flex gap-3">
          <div className="flex-1">
            <Field label="Category">
              <TextInput
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                placeholder="devops"
              />
            </Field>
          </div>
          <div className="flex-1">
            <Field label="Name">
              <TextInput
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="restart-nginx"
              />
            </Field>
          </div>
        </div>
        <Field label="SKILL.md">
          <TextArea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            rows={16}
          />
        </Field>
        <div className="mt-3 flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            onClick={submit}
            disabled={saving || !category.trim() || !name.trim()}
          >
            {saving ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function SkillsSection() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [editing, setEditing] = useState<Skill | null>(null);
  const [showEditor, setShowEditor] = useState(false);

  const load = () => api.listSkills().then(setSkills);
  useEffect(() => {
    load();
  }, []);

  const byCategory = useMemo(() => {
    const map = new Map<string, Skill[]>();
    for (const s of skills) {
      if (!map.has(s.category)) map.set(s.category, []);
      map.get(s.category)!.push(s);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [skills]);

  return (
    <div>
      <SectionHeader
        title="Skills"
        description="Reusable, category-organized playbooks (SKILL.md) the agent can load on demand. It can also create its own with the skill tool."
        action={
          <Button
            variant="primary"
            onClick={() => {
              setEditing(null);
              setShowEditor(true);
            }}
          >
            <PlusIcon size={13} /> New
          </Button>
        }
      />

      <SkillScannerCard />

      {skills.length === 0 && (
        <Card className="text-center text-xs text-gray-500">No skills yet.</Card>
      )}

      <div className="space-y-4">
        {byCategory.map(([cat, items]) => (
          <div key={cat}>
            <div className="mb-1.5 text-xs font-semibold uppercase tracking-wider text-gray-500">
              {cat}
            </div>
            <div className="space-y-2">
              {items.map((s) => (
                <Card key={`${s.category}/${s.name}`} className="flex items-center gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm text-gray-200">{s.name}</div>
                    {s.description && (
                      <div className="truncate text-xs text-gray-500">
                        {s.description}
                      </div>
                    )}
                  </div>
                  <Button
                    onClick={() => {
                      setEditing(s);
                      setShowEditor(true);
                    }}
                  >
                    Edit
                  </Button>
                  <Button
                    variant="danger"
                    onClick={async () => {
                      if (
                        await dialog.confirm({
                          title: "Delete skill",
                          message: `Delete skill "${s.category}/${s.name}"?`,
                          danger: true,
                          confirmText: "Delete",
                        })
                      ) {
                        await api.deleteSkill(s.category, s.name);
                        load();
                      }
                    }}
                  >
                    <TrashIcon size={14} />
                  </Button>
                </Card>
              ))}
            </div>
          </div>
        ))}
      </div>

      {showEditor && (
        <SkillEditor
          initial={editing}
          onClose={() => setShowEditor(false)}
          onSaved={load}
        />
      )}
    </div>
  );
}

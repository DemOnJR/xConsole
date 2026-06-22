import { useEffect, useMemo, useState } from "react";
import { api, type Skill } from "../../../lib/tauri";
import { PlusIcon, TrashIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, TextArea, TextInput } from "../ui";

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
      <div className="w-[min(680px,92vw)] rounded-xl border border-[#1f2737] bg-[#0d121b] p-5 shadow-2xl">
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
                      if (confirm(`Delete skill "${s.category}/${s.name}"?`)) {
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

import { useEffect, useState } from "react";
import { api } from "../../../lib/tauri";
import { Card, DocEditor, Field, SectionHeader } from "../ui";

export function MemorySection() {
  const [memory, setMemory] = useState("");
  const [user, setUser] = useState("");

  const load = () =>
    api.getAgentDocs().then((d) => {
      setMemory(d.memory);
      setUser(d.user);
    });

  useEffect(() => {
    load();
  }, []);

  return (
    <div>
      <SectionHeader
        title="Memory"
        description="Compact, persistent memory injected into every session. The agent also writes here via its memory tool. Keep entries terse; do not store secrets."
      />

      <Card className="mb-3">
        <Field
          label="Persistent memory (MEMORY.md)"
          hint="Durable facts: server roles, conventions, recurring fixes."
        >
          <DocEditor
            value={memory}
            rows={12}
            placeholder="- web-1 runs nginx + the marketing site"
            onSave={async (next) => {
              await api.saveMemoryDoc(next);
              setMemory(next);
            }}
          />
        </Field>
      </Card>

      <Card>
        <Field
          label="User profile (USER.md)"
          hint="Who you are and how you like the agent to work with you."
        >
          <DocEditor
            value={user}
            rows={8}
            placeholder="- Prefer concise answers and minimal, reversible changes."
            onSave={async (next) => {
              await api.saveUserDoc(next);
              setUser(next);
            }}
          />
        </Field>
      </Card>
    </div>
  );
}

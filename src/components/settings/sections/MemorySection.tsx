import { useEffect, useState } from "react";
import { api } from "../../../lib/tauri";
import { Card, DocEditor, Field, SectionHeader } from "../ui";

export function MemorySection() {
  const [memory, setMemory] = useState("");
  const [user, setUser] = useState("");
  const [taste, setTaste] = useState("");

  const load = () =>
    api.getAgentDocs().then((d) => {
      setMemory(d.memory);
      setUser(d.user);
      setTaste(d.taste ?? "");
    });

  useEffect(() => {
    load();
  }, []);

  return (
    <div>
      <SectionHeader
        title="Memory"
        description="Persistent knowledge injected every session. MEMORY is facts; USER is who you are; TASTE is how you like ops done. Keep entries terse; never store secrets."
      />

      <Card className="mb-3">
        <Field
          label="Persistent memory (MEMORY.md)"
          hint="Durable facts: server roles, conventions, recurring fixes."
        >
          <DocEditor
            value={memory}
            rows={10}
            placeholder="- web-1 runs nginx + the marketing site"
            onSave={async (next) => {
              await api.saveMemoryDoc(next);
              setMemory(next);
            }}
          />
        </Field>
      </Card>

      <Card className="mb-3">
        <Field
          label="User profile (USER.md)"
          hint="Who you are and how you like the agent to work with you."
        >
          <DocEditor
            value={user}
            rows={6}
            placeholder="- Prefer concise answers and minimal, reversible changes."
            onSave={async (next) => {
              await api.saveUserDoc(next);
              setUser(next);
            }}
          />
        </Field>
      </Card>

      <Card>
        <Field
          label="Working style (TASTE.md)"
          hint="Ops preferences the agent should follow (restarts, approvals, verbosity)."
        >
          <DocEditor
            value={taste}
            rows={6}
            placeholder={"- Prefer systemctl restart over docker-compose down/up\n- Never apt upgrade without approval\n- Keep replies terse"}
            onSave={async (next) => {
              await api.saveTasteDoc(next);
              setTaste(next);
            }}
          />
        </Field>
      </Card>
    </div>
  );
}

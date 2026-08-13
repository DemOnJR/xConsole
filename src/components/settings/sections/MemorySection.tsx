import { useEffect, useState } from "react";
import { api } from "../../../lib/tauri";
import { Card, DocEditor, Field, SectionHeader } from "../ui";

export function MemorySection() {
  const [memory, setMemory] = useState("");
  const [taste, setTaste] = useState("");

  const load = () =>
    api.getAgentDocs().then((d) => {
      setMemory(d.memory);
      setTaste(d.taste ?? "");
    });

  useEffect(() => {
    load();
  }, []);

  return (
    <div>
      <SectionHeader
        title="Memory"
        description="Persistent knowledge injected every session. MEMORY is facts; TASTE is who you are and how you like ops done (user profile + working style merged). Keep entries terse; never store secrets."
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

      <Card>
        <Field
          label="Preferences (TASTE.md)"
          hint="Your profile + how you like ops done: restarts, approvals, verbosity, preferred tools."
        >
          <DocEditor
            value={taste}
            rows={10}
            placeholder={
              "- Prefer systemctl restart over docker-compose down/up\n- Never apt upgrade without approval\n- Keep replies terse"
            }
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

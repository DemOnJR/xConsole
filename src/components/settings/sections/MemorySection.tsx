import { useEffect, useState } from "react";
import { api } from "../../../lib/tauri";
import { Card, DocEditor, Field, SectionHeader } from "../ui";

export function MemorySection() {
  const [memory, setMemory] = useState("");
  const [taste, setTaste] = useState("");

  const load = () =>
    api
      .getAgentDocs()
      .then((d) => {
        setMemory(d.memory);
        setTaste(d.taste ?? "");
      })
      .catch(() => {
        /* non-fatal: keep whatever we had */
      });

  useEffect(() => {
    load();
    // Re-fetch when the section regains focus: the agent appends to MEMORY.md /
    // TASTE.md mid-session, so a stale editor buffer must not clobber those
    // appends when the user saves.
    const onFocus = () => load();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
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
              // Merge with the live file instead of overwriting blindly: the agent
              // may have appended bullets since we loaded.
              const live = await api.getAgentDocs().catch(() => null);
              const merged = live ? live.memory : memory;
              const finalNext =
                merged !== memory ? `${next}\n${merged.replace(memory, "").trim()}` : next;
              await api.saveMemoryDoc(finalNext);
              setMemory(finalNext);
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
              // Same merge: preserve any agent-appended [taste] bullets.
              const live = await api.getAgentDocs().catch(() => null);
              const merged = live ? live.taste : taste;
              const finalNext =
                merged !== taste ? `${next}\n${merged.replace(taste, "").trim()}` : next;
              await api.saveTasteDoc(finalNext);
              setTaste(finalNext);
            }}
          />
        </Field>
      </Card>
    </div>
  );
}

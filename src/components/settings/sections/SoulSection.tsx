import { useEffect, useState } from "react";
import { api } from "../../../lib/tauri";
import { Card, DocEditor, SectionHeader } from "../ui";

export function SoulSection() {
  const [soul, setSoul] = useState("");

  useEffect(() => {
    api.getAgentDocs().then((d) => setSoul(d.soul));
  }, []);

  return (
    <div>
      <SectionHeader
        title="Soul"
        description="The agent's core identity (SOUL.md), loaded first in the system prompt. Edit it to shape the agent's persona, priorities, and operating style."
      />
      <Card>
        <DocEditor
          value={soul}
          rows={18}
          placeholder="You are the xConsole Agent..."
          onSave={async (next) => {
            await api.saveSoul(next);
            setSoul(next);
          }}
        />
      </Card>
    </div>
  );
}

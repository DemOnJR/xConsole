import { useEffect, useState } from "react";
import { api, type KnownHost } from "../../../lib/tauri";
import { Button, Card, SectionHeader } from "../ui";
import { TrashIcon } from "../../icons";

export function SecuritySection() {
  const [hosts, setHosts] = useState<KnownHost[]>([]);

  const load = () => api.listKnownHosts().then(setHosts);
  useEffect(() => {
    load();
  }, []);

  const forget = async (h: KnownHost) => {
    if (!confirm(`Forget pinned key for ${h.host}:${h.port}?`)) return;
    await api.forgetHostKey(h.host, h.port);
    load();
  };

  return (
    <div>
      <SectionHeader
        title="Security"
        description="Pinned SSH host keys (trust-on-first-use). Forget a key only after a legitimate server key rotation."
      />

      {hosts.length === 0 && (
        <Card className="text-center text-xs text-gray-500">
          No pinned hosts yet.
        </Card>
      )}

      <div className="space-y-2">
        {hosts.map((h) => (
          <Card key={`${h.host}:${h.port}`} className="flex items-center gap-3">
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm text-gray-200">
                {h.host}:{h.port}
              </div>
              <div className="truncate font-mono text-[11px] text-gray-500">
                {h.key_type} · {h.fingerprint}
              </div>
            </div>
            <Button variant="danger" onClick={() => forget(h)} title="Forget key">
              <TrashIcon size={14} />
            </Button>
          </Card>
        ))}
      </div>
    </div>
  );
}

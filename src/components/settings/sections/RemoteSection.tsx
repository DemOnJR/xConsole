import { useCallback, useEffect, useState } from "react";
import { api, type RemoteStatus } from "../../../lib/tauri";
import { useVpsStore } from "../../../stores/vpsStore";
import { Button, Field, SectionHeader, Select, TextInput, Toggle } from "../ui";

/**
 * Remote control over Discord.
 *
 * The dangerous setting here is the allowlist, so the UI is built around it: the
 * bridge cannot be armed without one, and the state of the arming is stated plainly
 * rather than left to be inferred from a toggle.
 */
export function RemoteSection() {
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const vpsList = useVpsStore((s) => s.vpsList);

  // Local draft so the form does not fight the round trip on every keystroke.
  const [draft, setDraft] = useState({
    enabled: false,
    channelId: "",
    allowedUserIds: "",
    prefix: "!x",
    safetyMode: "allowlist",
    targets: [] as string[],
  });

  const load = useCallback(async () => {
    const s = await api.getRemoteStatus().catch(() => null);
    if (!s) return;
    setStatus(s);
    setDraft({
      enabled: s.enabled,
      channelId: s.channel_id,
      allowedUserIds: s.allowed_user_ids,
      prefix: s.prefix,
      safetyMode: s.safety_mode,
      targets: s.targets,
    });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const next = await api.saveRemoteConfig({ ...draft, token: token || null });
      setStatus(next);
      setToken("");
      setSavedAt(Date.now());
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const toggleTarget = (id: string) =>
    setDraft((d) => ({
      ...d,
      targets: d.targets.includes(id)
        ? d.targets.filter((t) => t !== id)
        : [...d.targets, id],
    }));

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Remote control"
        description="Command the agent from Discord while xConsole is running. It polls outbound only — no port is opened and nothing can reach this machine from the internet. Close xConsole and remote control stops."
      />

      {/* Colour used only where it reports something the user must act on. */}
      {status && (
        <div
          className={`border px-3 py-2 text-[11px] ${
            status.usable
              ? "border-[var(--border-strong)] bg-[var(--surface)] text-gray-300"
              : "border-[var(--warning)]/40 bg-[var(--warning)]/10 text-[var(--warning)]"
          }`}
        >
          {status.usable
            ? "Armed — an allowed user can command the agent from the configured channel."
            : !status.has_token
              ? "Not armed: no bot token saved."
              : !status.enabled
                ? "Not armed: remote control is off."
                : "Not armed: a channel id and at least one allowed user id are required."}
        </div>
      )}

      {error && (
        <div className="border border-red-500/40 bg-red-500/10 px-3 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div className="grid gap-4 md:grid-cols-2">
        <Field
          label="Bot token"
          hint={
            status?.has_token
              ? "A token is saved in your OS keychain. Type a new one to replace it; leave blank to keep it."
              : "From the Discord developer portal. Stored in your OS keychain, never shown again."
          }
        >
          <TextInput
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder={status?.has_token ? "•••••••• (saved)" : "Bot token"}
            autoComplete="off"
          />
        </Field>

        <Field label="Channel id" hint="Only this channel is read. Being added to another grants nothing.">
          <TextInput
            value={draft.channelId}
            onChange={(e) => setDraft((d) => ({ ...d, channelId: e.target.value }))}
            placeholder="123456789012345678"
          />
        </Field>
      </div>

      <Field
        label="Who may command it"
        hint="Discord user ids, comma separated. This is the whole security boundary — anyone listed here can run commands on your servers. An empty list authorises nobody."
      >
        <TextInput
          value={draft.allowedUserIds}
          onChange={(e) => setDraft((d) => ({ ...d, allowedUserIds: e.target.value }))}
          placeholder="your Discord user id"
        />
      </Field>

      <div className="grid gap-4 md:grid-cols-2">
        <Field label="Command prefix" hint="Only messages starting with this are treated as commands. Blank means every message is.">
          <TextInput
            value={draft.prefix}
            onChange={(e) => setDraft((d) => ({ ...d, prefix: e.target.value }))}
            placeholder="!x"
          />
        </Field>

        <Field
          label="Trust for remote commands"
          hint="Nobody can answer an approval prompt from a phone, so a command needing one stops rather than waiting."
        >
          <Select
            value={draft.safetyMode}
            onChange={(e) => setDraft((d) => ({ ...d, safetyMode: e.target.value }))}
          >
            <option value="approve">Ask every command (nothing will run remotely)</option>
            <option value="allowlist">Auto-run read-only commands</option>
            <option value="full">Run anything</option>
          </Select>
        </Field>
      </div>

      <Field label="Servers it may touch" hint="Remote commands are limited to these.">
        {vpsList.length === 0 ? (
          <p className="text-[11px] text-gray-500">No servers configured yet.</p>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {vpsList.map((v) => {
              const on = draft.targets.includes(v.id);
              return (
                <button
                  key={v.id}
                  type="button"
                  onClick={() => toggleTarget(v.id)}
                  className={`border px-2 py-1 text-[11px] transition ${
                    on
                      ? "border-[var(--accent)] bg-[var(--accent-muted)] text-gray-100"
                      : "border-[var(--border)] text-gray-400 hover:border-[var(--border-strong)]"
                  }`}
                >
                  {v.name}
                </button>
              );
            })}
          </div>
        )}
      </Field>

      <div className="flex items-center gap-3 border-t border-[var(--border)] pt-4">
        <Toggle
          checked={draft.enabled}
          onChange={(v) => setDraft((d) => ({ ...d, enabled: v }))}
          label={draft.enabled ? "Remote control on" : "Remote control off"}
        />
        <div className="ml-auto flex items-center gap-3">
          {savedAt && <span className="font-mono text-[11px] text-gray-500">Saved</span>}
          <Button variant="primary" onClick={save} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </Button>
          {status?.has_token && (
            <Button
              variant="danger"
              onClick={async () => {
                await api.clearRemoteToken();
                await load();
              }}
            >
              Remove token
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type RemoteKind,
  type RemoteStatus,
  type WhatsAppStatus,
} from "../../../lib/tauri";
import { useVpsStore } from "../../../stores/vpsStore";
import { RefreshIcon } from "../../icons";
import { Button, Card, Field, SectionHeader, Select, TextInput, Toggle } from "../ui";

/**
 * Remote control over Discord, Telegram or WhatsApp.
 *
 * The dangerous setting is the allowlist, so the UI is built around it: no bridge can
 * be armed without one, and the state of the arming is stated plainly per transport
 * rather than left to be inferred from a toggle.
 *
 * The three are presented in order of how hard they are to set up, because that is the
 * question a user actually has when they arrive here. WhatsApp is a QR scan, Telegram
 * is one chat with BotFather, Discord is a developer-portal application.
 *
 * One at a time, behind tabs: three stacked cards is a long scroll of fields for two
 * platforms you are not setting up. The tabs carry each transport's state as a dot, so
 * hiding two of them does not hide whether they are armed — which is the one thing this
 * screen exists to say. Every draft is kept mounted-or-not in `drafts`, so Save still
 * saves all three; switching tabs is a view change, never an edit.
 */

type Draft = {
  enabled: boolean;
  chatId: string;
  allowedUserIds: string;
  token: string;
};

const BLANK: Draft = { enabled: false, chatId: "", allowedUserIds: "", token: "" };

/** Copy that differs per platform. Kept together so the three cards cannot drift. */
const COPY: Record<
  RemoteKind,
  {
    name: string;
    setup: string;
    chatLabel: string;
    chatHint: string;
    allowLabel: string;
    allowHint: string;
    allowPlaceholder: string;
  }
> = {
  whatsapp: {
    name: "WhatsApp",
    setup: "Scan a QR code with your phone. No account to register, nothing to paste.",
    chatLabel: "Restrict to one chat",
    chatHint: "Optional. A phone number or group id — leave blank to accept any chat an allowed person writes from.",
    allowLabel: "Who may command it",
    allowHint:
      "Phone numbers in international form (+40 712 345 678), or @usernames. Comma separated. This is the whole security boundary — anyone listed here can run commands on your servers.",
    allowPlaceholder: "+40712345678, @ada.lovelace",
  },
  telegram: {
    name: "Telegram",
    setup: "Message @BotFather, send /newbot, and paste the token it gives you.",
    chatLabel: "Restrict to one chat",
    chatHint: "Optional. A chat id — leave blank to accept a direct message from anyone allowed below.",
    allowLabel: "Who may command it",
    allowHint:
      "Telegram user ids, or @usernames. Comma separated. This is the whole security boundary — anyone listed here can run commands on your servers.",
    allowPlaceholder: "123456789, @ada",
  },
  discord: {
    name: "Discord",
    setup:
      "Create an application in the Discord developer portal, add a bot, invite it to your server, and paste its token.",
    chatLabel: "Channel id",
    chatHint: "Required. Only this channel is read — being added to another grants nothing.",
    allowLabel: "Who may command it",
    allowHint:
      "Discord user ids, comma separated. This is the whole security boundary — anyone listed here can run commands on your servers.",
    allowPlaceholder: "your Discord user id",
  },
};

/** Setup order, easiest first — the order someone choosing between them wants. */
const ORDER: RemoteKind[] = ["whatsapp", "telegram", "discord"];

/** Mirrors `SETTING_SIDECAR` in src-tauri/src/ai/remote/whatsapp.rs. */
const WHATSAPP_SIDECAR_SETTING = "remote.whatsapp.sidecar_path";

export function RemoteSection() {
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const vpsList = useVpsStore((s) => s.vpsList);

  // Local drafts so the form does not fight the round trip on every keystroke.
  const [shared, setShared] = useState({
    enabled: false,
    prefix: "!x",
    safetyMode: "allowlist",
    targets: [] as string[],
  });
  const [drafts, setDrafts] = useState<Record<RemoteKind, Draft>>({
    discord: { ...BLANK },
    telegram: { ...BLANK },
    whatsapp: { ...BLANK },
  });
  const [tab, setTab] = useState<RemoteKind>("whatsapp");

  const load = useCallback(async () => {
    const s = await api.getRemoteStatus().catch(() => null);
    if (!s) return;
    setStatus(s);
    setShared({
      enabled: s.enabled,
      prefix: s.prefix,
      safetyMode: s.safety_mode,
      targets: s.targets,
    });
    setDrafts((prev) => {
      const next = { ...prev };
      for (const t of s.transports) {
        next[t.kind] = {
          enabled: t.enabled,
          chatId: t.chat_id,
          allowedUserIds: t.allowed_user_ids,
          // Never repopulated from the server — it is never sent there.
          token: "",
        };
      }
      return next;
    });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const next = await api.saveRemoteConfig(
        shared,
        ORDER.map((kind) => ({
          kind,
          enabled: drafts[kind].enabled,
          chatId: drafts[kind].chatId,
          allowedUserIds: drafts[kind].allowedUserIds,
          token: drafts[kind].token || null,
        })),
      );
      setStatus(next);
      setDrafts((d) => ({
        discord: { ...d.discord, token: "" },
        telegram: { ...d.telegram, token: "" },
        whatsapp: { ...d.whatsapp, token: "" },
      }));
      setSavedAt(Date.now());
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const toggleTarget = (id: string) =>
    setShared((s) => ({
      ...s,
      targets: s.targets.includes(id) ? s.targets.filter((t) => t !== id) : [...s.targets, id],
    }));

  const patch = (kind: RemoteKind, p: Partial<Draft>) =>
    setDrafts((d) => ({ ...d, [kind]: { ...d[kind], ...p } }));

  const armed = status?.transports.filter((t) => t.usable) ?? [];
  // Guarded rather than cast: the backend could name a transport this build has no copy
  // for, and an undefined lookup would take the whole settings screen down.
  const lastRoute = ORDER.find((k) => k === status?.last_route);

  return (
    <div className="space-y-6">
      <SectionHeader
        title="Remote control"
        description="Command the agent from WhatsApp, Telegram or Discord while xConsole is running. It polls outbound only — no port is opened and nothing can reach this machine from the internet. Close xConsole and remote control stops."
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
            ? `Armed on ${armed.map((t) => COPY[t.kind].name).join(", ")} — an allowed person can command the agent.`
            : !status.enabled
              ? "Not armed: remote control is off."
              : "Not armed: no transport is fully configured yet."}
          {status.usable && lastRoute && (
            <>
              {" "}
              The conversation is on {COPY[lastRoute].name}; ask it to carry on somewhere
              else and it will answer there.
            </>
          )}
        </div>
      )}

      {error && (
        <div className="border border-red-500/40 bg-red-500/10 px-3 py-2 text-[11px] text-red-300">
          {error}
        </div>
      )}

      <div>
        <div className="flex border-b border-[var(--border)]">
          {ORDER.map((kind) => {
            const t = status?.transports.find((x) => x.kind === kind);
            const active = tab === kind;
            // Armed, switched on but not finished, or off. Colour only where it reports
            // something: a grey dot is not a problem, an amber one is unfinished work.
            const state = t?.usable
              ? { colour: "bg-[var(--success)]", why: "armed" }
              : drafts[kind].enabled
                ? { colour: "bg-[var(--warning)]", why: "on, but not fully configured" }
                : { colour: "bg-[var(--border-strong)]", why: "off" };
            return (
              <button
                key={kind}
                type="button"
                onClick={() => setTab(kind)}
                aria-selected={active}
                className={`flex items-center gap-2 border-b-2 px-4 py-2.5 text-xs font-medium transition ${
                  active
                    ? "border-[var(--accent)] text-gray-100"
                    : "border-transparent text-gray-400 hover:text-gray-200"
                }`}
              >
                {COPY[kind].name}
                <span
                  className={`h-1.5 w-1.5 shrink-0 ${state.colour}`}
                  data-tooltip={`${COPY[kind].name}: ${state.why}`}
                />
              </button>
            );
          })}
        </div>

        {/* Only the selected one is mounted; the other two keep their drafts in state
            and are still written by Save. */}
        <div className="pt-4">
          <TransportCard
            // Keyed, so a token-test result never survives into another platform's card.
            key={tab}
            kind={tab}
            draft={drafts[tab]}
            status={status?.transports.find((t) => t.kind === tab)}
            onChange={(p) => patch(tab, p)}
            onReload={load}
          />
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Field
          label="Command prefix"
          hint="Only messages starting with this are treated as commands. Blank means every message is. Shared by all three."
        >
          <TextInput
            value={shared.prefix}
            onChange={(e) => setShared((s) => ({ ...s, prefix: e.target.value }))}
            placeholder="!x"
          />
        </Field>

        <Field
          label="Trust for remote commands"
          hint="Nobody can answer an approval prompt from a phone, so a command needing one stops rather than waiting."
        >
          <Select
            value={shared.safetyMode}
            onChange={(e) => setShared((s) => ({ ...s, safetyMode: e.target.value }))}
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
              const on = shared.targets.includes(v.id);
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

      {/* The thread is shared by every transport and outlives restarts, so it needs to be
          visible and clearable — a persistent conversation nobody can see or reset is a
          surprise, not a feature. */}
      <Field
        label="Conversation"
        hint="All three apps share one thread, so a follow-up makes sense wherever you type it — ask about a server on Telegram, say “restart it” on WhatsApp. Everyone on an allowlist shares it."
      >
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-gray-400">
            {status && status.conversation_len > 0
              ? `${status.conversation_len} message${status.conversation_len === 1 ? "" : "s"}${
                  lastRoute ? `, last on ${COPY[lastRoute].name}` : ""
                }.`
              : "Nothing yet."}
          </span>
          {status && status.conversation_len > 0 && (
            <Button
              onClick={async () => {
                await api.resetRemoteConversation().then(setStatus).catch((e) => setError(String(e)));
              }}
            >
              Start a new one
            </Button>
          )}
        </div>
      </Field>

      <div className="flex items-center gap-3 border-t border-[var(--border)] pt-4">
        <Toggle
          checked={shared.enabled}
          onChange={(v) => setShared((s) => ({ ...s, enabled: v }))}
          label={shared.enabled ? "Remote control on" : "Remote control off"}
        />
        <div className="ml-auto flex items-center gap-3">
          {savedAt && <span className="font-mono text-[11px] text-gray-500">Saved</span>}
          <Button variant="primary" onClick={save} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </div>
      </div>
    </div>
  );
}

function TransportCard({
  kind,
  draft,
  status,
  onChange,
  onReload,
}: {
  kind: RemoteKind;
  draft: Draft;
  status?: { usable: boolean; has_token: boolean; needs_token: boolean };
  onChange: (p: Partial<Draft>) => void;
  onReload: () => void;
}) {
  const copy = COPY[kind];
  const [check, setCheck] = useState<string | null>(null);

  return (
    <Card>
      <div className="flex items-center gap-3">
        <span className="text-[12px] font-medium text-gray-200">{copy.name}</span>
        {status?.usable && (
          <span className="border border-[var(--border-strong)] px-1.5 py-0.5 font-mono text-[10px] text-gray-400">
            armed
          </span>
        )}
        <div className="ml-auto">
          <Toggle
            checked={draft.enabled}
            onChange={(v) => onChange({ enabled: v })}
            label={draft.enabled ? "On" : "Off"}
          />
        </div>
      </div>
      <p className="mt-1 text-[11px] text-gray-500">{copy.setup}</p>

      <div className="mt-4 space-y-4">
        {kind === "whatsapp" ? (
          <WhatsAppLink onReload={onReload} />
        ) : (
          <Field
            label="Bot token"
            hint={
              status?.has_token
                ? "A token is saved in your OS keychain. Type a new one to replace it; leave blank to keep it."
                : "Stored in your OS keychain, never shown again."
            }
          >
            <div className="flex items-center gap-2">
              <TextInput
                type="password"
                value={draft.token}
                onChange={(e) => onChange({ token: e.target.value })}
                placeholder={status?.has_token ? "•••••••• (saved)" : "Bot token"}
                autoComplete="off"
              />
              {status?.has_token && kind === "telegram" && (
                <Button
                  onClick={async () => {
                    setCheck("Checking…");
                    // Answers "is my token right?" with a fact instead of a silent
                    // bridge that never replies.
                    await api
                      .testRemoteToken(kind)
                      .then(setCheck)
                      .catch((e) => setCheck(String(e)));
                  }}
                >
                  Test
                </Button>
              )}
              {status?.has_token && (
                <Button
                  variant="danger"
                  onClick={async () => {
                    await api.clearRemoteToken(kind);
                    onReload();
                  }}
                >
                  Remove
                </Button>
              )}
            </div>
            {check && <p className="mt-1 font-mono text-[11px] text-gray-400">{check}</p>}
          </Field>
        )}

        <Field label={copy.chatLabel} hint={copy.chatHint}>
          <TextInput
            value={draft.chatId}
            onChange={(e) => onChange({ chatId: e.target.value })}
            placeholder={kind === "discord" ? "123456789012345678" : "optional"}
          />
        </Field>

        <Field label={copy.allowLabel} hint={copy.allowHint}>
          <TextInput
            value={draft.allowedUserIds}
            onChange={(e) => onChange({ allowedUserIds: e.target.value })}
            placeholder={copy.allowPlaceholder}
          />
        </Field>
      </div>
    </Card>
  );
}

/**
 * WhatsApp pairing.
 *
 * The QR arrives as an event because pairing takes as long as the user takes to find
 * their phone, and WhatsApp rotates the code every twenty seconds — so this listens
 * rather than polls, and the code on screen is always the live one.
 */
function WhatsAppLink({ onReload }: { onReload: () => void }) {
  const [wa, setWa] = useState<WhatsAppStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const unlisten = useRef<(() => void) | null>(null);

  useEffect(() => {
    void api.whatsappStatus().then(setWa).catch(() => {});
    void listen<WhatsAppStatus>("remote://whatsapp", (e) => setWa(e.payload)).then((un) => {
      unlisten.current = un;
    });
    return () => unlisten.current?.();
  }, []);

  return (
    <div className="space-y-3">
      {wa?.building ? (
        <div className="flex items-center gap-3 border border-[var(--accent)]/30 bg-[var(--accent-muted)]/10 px-3.5 py-3">
          <RefreshIcon className="animate-spin text-[var(--accent)]" size={16} />
          <div className="space-y-0.5">
            <p className="text-[12px] font-medium text-gray-200">
              {wa.build_step || "Preparing WhatsApp helper…"}
            </p>
            <p className="text-[11px] text-gray-400">
              Setting up everything automatically. This only happens once.
            </p>
          </div>
        </div>
      ) : wa?.linked ? (
        <div className="flex items-center gap-3">
          <div className="text-[11px] text-gray-300">
            Linked{wa.phone ? ` as ${wa.phone}` : ""}
            {wa.push_name ? ` (${wa.push_name})` : ""}.{" "}
            <span className={wa.connected ? "text-gray-500" : "text-[var(--warning)]"}>
              {wa.connected ? "Connected." : "Reconnecting…"}
            </span>
          </div>
          <Button
            variant="danger"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              // Unpairs on WhatsApp's side too, so no stale "xConsole" device is left
              // in the phone's linked-devices list for the user to hunt down.
              await api.whatsappUnlink().then(setWa).catch(() => {});
              setBusy(false);
              onReload();
            }}
          >
            Unlink
          </Button>
        </div>
      ) : wa?.qr_svg ? (
        <div className="flex items-start gap-4">
          {/* The QR is rendered by the Rust side, from the exact bytes WhatsApp sent. */}
          <div
            className="border border-[var(--border)] bg-white p-2 [&>svg]:block"
            // eslint-disable-next-line react/no-danger -- generated locally, never user input
            dangerouslySetInnerHTML={{ __html: wa.qr_svg }}
          />
          <div className="space-y-2 text-[11px] text-gray-400">
            <p>On your phone: WhatsApp, Settings, Linked devices, Link a device.</p>
            <p className="text-gray-500">The code refreshes every few seconds until you scan it.</p>
            <Button
              onClick={async () => {
                await api.whatsappLinkCancel().then(setWa).catch(() => {});
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex items-center gap-3">
          <Button
            variant="primary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              await api.whatsappLinkStart().then(setWa).catch(() => {});
              setBusy(false);
            }}
          >
            {busy ? "Starting…" : "Link with QR code"}
          </Button>
          <span className="text-[11px] text-gray-500">
            xConsole appears in your phone's linked devices, and can be revoked there.
          </span>
        </div>
      )}

      {wa?.error && (
        <div className="border border-[var(--warning)]/40 bg-[var(--warning)]/10 px-3 py-2 text-[11px] text-[var(--warning)]">
          {wa.error}
        </div>
      )}

      <div className="pt-2">
        <button
          type="button"
          onClick={() => setShowAdvanced(!showAdvanced)}
          className="text-[10px] text-gray-500 hover:text-gray-400 transition underline underline-offset-2"
        >
          {showAdvanced ? "Hide custom binary path" : "Custom helper binary…"}
        </button>
        {showAdvanced && (
          <div className="mt-2 flex items-center gap-3 border border-[var(--border)] bg-[var(--surface)] p-2.5">
            <Button
              disabled={busy}
              onClick={async () => {
                const path = await api.pickFile("Locate the WhatsApp helper").catch(() => null);
                if (!path) return;
                setBusy(true);
                await api.setSetting(WHATSAPP_SIDECAR_SETTING, path).catch(() => {});
                await api.whatsappStatus().then(setWa).catch(() => {});
                setBusy(false);
              }}
            >
              {busy ? "Checking…" : "Choose helper binary…"}
            </Button>
            <span className="text-[11px] text-gray-400">
              Optional override. xConsole builds and manages the helper automatically.
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

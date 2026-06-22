import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { api, type RemoteFileStat, type SftpEntry } from "../lib/tauri";
import {
  bitsToRwx,
  octalToTriplets,
  parseModeInput,
  rwxToBits,
  tripletsToOctal,
  type RwxTriplet,
} from "../lib/filePermissions";

interface Props {
  entry: SftpEntry;
  vpsId: string;
  onClose: () => void;
  onApplied: () => void;
}

function RwxRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: RwxTriplet;
  onChange: (v: RwxTriplet) => void;
}) {
  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="w-14 text-gray-500">{label}</span>
      {(["r", "w", "x"] as const).map((bit) => (
        <label key={bit} className="flex items-center gap-1 text-gray-300">
          <input
            type="checkbox"
            checked={value[bit]}
            onChange={(e) => onChange({ ...value, [bit]: e.target.checked })}
            className="rounded border-[#1f2737]"
          />
          {bit.toUpperCase()}
        </label>
      ))}
    </div>
  );
}

export function SftpPermissionsDialog({ entry, vpsId, onClose, onApplied }: Props) {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [owner, setOwner] = useState("");
  const [group, setGroup] = useState("");
  const [recursive, setRecursive] = useState(entry.is_dir);
  const [userRwx, setUserRwx] = useState<RwxTriplet>({ r: true, w: true, x: true });
  const [groupRwx, setGroupRwx] = useState<RwxTriplet>({ r: true, w: false, x: true });
  const [otherRwx, setOtherRwx] = useState<RwxTriplet>({ r: true, w: false, x: true });
  const [modeInput, setModeInput] = useState("755");

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const stat: RemoteFileStat = await api.vpsFileStat(vpsId, entry.path);
        if (!mounted) return;
        setOwner(stat.owner);
        setGroup(stat.group);
        setRecursive(stat.is_dir);
        const [u, g, o] = octalToTriplets(stat.mode);
        setUserRwx(bitsToRwx(u));
        setGroupRwx(bitsToRwx(g));
        setOtherRwx(bitsToRwx(o));
        setModeInput(tripletsToOctal(u, g, o));
      } catch (e) {
        if (mounted) setError(String(e));
      } finally {
        if (mounted) setLoading(false);
      }
    })();
    return () => {
      mounted = false;
    };
  }, [entry.path, vpsId]);

  const syncFromRwx = (u: RwxTriplet, g: RwxTriplet, o: RwxTriplet) => {
    setModeInput(tripletsToOctal(rwxToBits(u), rwxToBits(g), rwxToBits(o)));
  };

  const applyModeInput = (raw: string) => {
    const parsed = parseModeInput(raw);
    if (!parsed) return;
    const [u, g, o] = octalToTriplets(parsed);
    setUserRwx(bitsToRwx(u));
    setGroupRwx(bitsToRwx(g));
    setOtherRwx(bitsToRwx(o));
    setModeInput(parsed);
  };

  const apply = async () => {
    setSaving(true);
    setError(null);
    const mode = parseModeInput(modeInput);
    if (!mode) {
      setError("Invalid octal mode");
      setSaving(false);
      return;
    }
    try {
      await api.vpsFileChmod(vpsId, entry.path, mode, recursive);
      if (owner.trim() && group.trim()) {
        await api.vpsFileChown(vpsId, entry.path, owner.trim(), group.trim(), recursive);
      }
      onApplied();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/50 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="w-full max-w-md rounded-lg border border-[#1f2737] bg-[#0b0f17] shadow-xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="border-b border-[#1f2737] px-4 py-2.5">
          <h3 className="text-sm font-medium text-gray-200">Properties</h3>
          <p className="mt-0.5 truncate font-mono text-[10px] text-gray-500">{entry.path}</p>
        </div>

        <div className="space-y-3 px-4 py-3">
          {loading ? (
            <p className="text-xs text-gray-500">Loading…</p>
          ) : (
            <>
              <div className="grid grid-cols-2 gap-2">
                <label className="text-[10px] text-gray-500">
                  Owner
                  <input
                    value={owner}
                    onChange={(e) => setOwner(e.target.value)}
                    className="mt-0.5 w-full rounded border border-[#1f2737] bg-[#11161f] px-2 py-1 text-xs text-gray-200"
                  />
                </label>
                <label className="text-[10px] text-gray-500">
                  Group
                  <input
                    value={group}
                    onChange={(e) => setGroup(e.target.value)}
                    className="mt-0.5 w-full rounded border border-[#1f2737] bg-[#11161f] px-2 py-1 text-xs text-gray-200"
                  />
                </label>
              </div>

              <div className="space-y-1.5 rounded border border-[#1f2737]/80 bg-[#11161f]/50 p-2">
                <RwxRow
                  label="Owner"
                  value={userRwx}
                  onChange={(v) => {
                    setUserRwx(v);
                    syncFromRwx(v, groupRwx, otherRwx);
                  }}
                />
                <RwxRow
                  label="Group"
                  value={groupRwx}
                  onChange={(v) => {
                    setGroupRwx(v);
                    syncFromRwx(userRwx, v, otherRwx);
                  }}
                />
                <RwxRow
                  label="Others"
                  value={otherRwx}
                  onChange={(v) => {
                    setOtherRwx(v);
                    syncFromRwx(userRwx, groupRwx, v);
                  }}
                />
              </div>

              <label className="flex items-center gap-2 text-xs text-gray-400">
                Octal
                <input
                  value={modeInput}
                  onChange={(e) => setModeInput(e.target.value)}
                  onBlur={() => applyModeInput(modeInput)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") applyModeInput(modeInput);
                  }}
                  className="w-16 rounded border border-[#1f2737] bg-[#0b0f17] px-2 py-0.5 font-mono text-xs text-gray-200"
                />
              </label>

              {entry.is_dir && (
                <label className="flex items-center gap-2 text-xs text-gray-400">
                  <input
                    type="checkbox"
                    checked={recursive}
                    onChange={(e) => setRecursive(e.target.checked)}
                  />
                  Apply recursively to all files and subfolders
                </label>
              )}

              <p className="text-[10px] text-gray-600">
                Changes run via SSH ({recursive ? "chmod/chown -R" : "chmod/chown"}).
              </p>
            </>
          )}

          {error && <p className="text-xs text-red-300">{error}</p>}
        </div>

        <div className="flex justify-end gap-2 border-t border-[#1f2737] px-4 py-2.5">
          <button
            type="button"
            className="rounded px-3 py-1 text-xs text-gray-400 hover:bg-[#1f2737]"
            onClick={onClose}
            disabled={saving}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded bg-cyan-700 px-3 py-1 text-xs text-white hover:bg-cyan-600 disabled:opacity-50"
            onClick={() => void apply()}
            disabled={loading || saving}
          >
            {saving ? "Applying…" : "OK"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

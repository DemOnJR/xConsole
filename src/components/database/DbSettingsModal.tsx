import { useEffect, useState } from "react";
import { api, type DbSavedConnection } from "../../lib/tauri";
import {
  CloseIcon,
  SettingsIcon,
  TrashIcon,
  SlidersIcon,
  ShieldIcon,
  KeyIcon,
  CheckIcon,
} from "../icons";

export interface DbSettings {
  pageSize: number;
  density: "compact" | "comfortable";
  fontSize: "xs" | "sm" | "base";
  refreshInterval: number;
  nullLabel: string;
  showRowNumbers: boolean;
  maxExportLimit: number;
  confirmDestructive: boolean;
}

export const DEFAULT_DB_SETTINGS: DbSettings = {
  pageSize: 200,
  density: "compact",
  fontSize: "xs",
  refreshInterval: 5,
  nullLabel: "NULL",
  showRowNumbers: true,
  maxExportLimit: 50000,
  confirmDestructive: true,
};

export function loadDbSettings(): DbSettings {
  try {
    const s = localStorage.getItem("xconsole-db-settings");
    if (s) {
      return { ...DEFAULT_DB_SETTINGS, ...JSON.parse(s) };
    }
  } catch {
    /* fallback to defaults */
  }
  return DEFAULT_DB_SETTINGS;
}

export function saveDbSettings(settings: DbSettings) {
  try {
    localStorage.setItem("xconsole-db-settings", JSON.stringify(settings));
  } catch {
    /* ignore storage quota */
  }
}

interface DbSettingsModalProps {
  open: boolean;
  vpsId: string;
  onClose: () => void;
  onSavedLoginsChanged?: () => void;
  onSettingsChanged?: (settings: DbSettings) => void;
}

export function DbSettingsModal({
  open,
  vpsId,
  onClose,
  onSavedLoginsChanged,
  onSettingsChanged,
}: DbSettingsModalProps) {
  const [tab, setTab] = useState<"general" | "query" | "saved">("general");
  const [settings, setSettings] = useState<DbSettings>(loadDbSettings);
  const [savedLogins, setSavedLogins] = useState<DbSavedConnection[]>([]);
  const [loadingLogins, setLoadingLogins] = useState(false);
  const [savedSuccess, setSavedSuccess] = useState(false);

  useEffect(() => {
    if (open) {
      setSettings(loadDbSettings());
      loadSaved();
    }
  }, [open, vpsId]);

  const loadSaved = async () => {
    setLoadingLogins(true);
    try {
      const list = await api.dbListConnections(vpsId);
      setSavedLogins(list);
    } catch {
      setSavedLogins([]);
    } finally {
      setLoadingLogins(false);
    }
  };

  const handleForgetLogin = async (id: string) => {
    try {
      await api.dbForgetConnection(id);
      await loadSaved();
      onSavedLoginsChanged?.();
    } catch {
      /* ignore */
    }
  };

  const updateSetting = <K extends keyof DbSettings>(key: K, value: DbSettings[K]) => {
    const next = { ...settings, [key]: value };
    setSettings(next);
    saveDbSettings(next);
    onSettingsChanged?.(next);
    setSavedSuccess(true);
    setTimeout(() => setSavedSuccess(false), 1500);
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex w-full max-w-lg flex-col overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-2xl text-[12px] animate-in fade-in zoom-in-95 duration-150">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-4 py-2.5">
          <div className="flex items-center gap-2 font-medium text-gray-200">
            <SettingsIcon size={15} className="text-violet-400" />
            <span>Database Manager Settings</span>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-gray-400 hover:bg-[var(--border)] hover:text-white"
            data-tooltip="Close"
          >
            <CloseIcon size={14} />
          </button>
        </div>

        {/* Tab switcher */}
        <div className="flex border-b border-[var(--border)] bg-[var(--bg)] px-3 pt-1 gap-1">
          <button
            onClick={() => setTab("general")}
            className={`flex items-center gap-1.5 px-3 py-1.5 border-b-2 font-medium transition-colors ${
              tab === "general"
                ? "border-violet-500 text-violet-300"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <SlidersIcon size={13} />
            <span>Display & Grid</span>
          </button>
          <button
            onClick={() => setTab("query")}
            className={`flex items-center gap-1.5 px-3 py-1.5 border-b-2 font-medium transition-colors ${
              tab === "query"
                ? "border-violet-500 text-violet-300"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <ShieldIcon size={13} />
            <span>SQL & Safety</span>
          </button>
          <button
            onClick={() => setTab("saved")}
            className={`flex items-center gap-1.5 px-3 py-1.5 border-b-2 font-medium transition-colors ${
              tab === "saved"
                ? "border-violet-500 text-violet-300"
                : "border-transparent text-gray-400 hover:text-gray-200"
            }`}
          >
            <KeyIcon size={13} />
            <span>Saved Logins ({savedLogins.length})</span>
          </button>
        </div>

        {/* Content */}
        <div className="max-h-[380px] overflow-y-auto p-4 space-y-4">
          {tab === "general" && (
            <div className="space-y-3.5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Default Page Size</div>
                  <div className="text-[10px] text-gray-500">
                    Number of rows fetched per page in data view
                  </div>
                </div>
                <select
                  value={settings.pageSize}
                  onChange={(e) => updateSetting("pageSize", Number(e.target.value))}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1 text-gray-200 outline-none focus:border-violet-500"
                >
                  <option value={25}>25 rows</option>
                  <option value={50}>50 rows</option>
                  <option value={100}>100 rows</option>
                  <option value={200}>200 rows (recommended)</option>
                  <option value={500}>500 rows</option>
                  <option value={1000}>1000 rows</option>
                </select>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Grid Density</div>
                  <div className="text-[10px] text-gray-500">
                    Row padding height for table data
                  </div>
                </div>
                <div className="flex rounded border border-[var(--border)] bg-[var(--bg)] p-0.5">
                  <button
                    type="button"
                    onClick={() => updateSetting("density", "compact")}
                    className={`rounded px-2.5 py-0.5 text-[11px] ${
                      settings.density === "compact"
                        ? "bg-violet-600 text-white font-medium"
                        : "text-gray-400 hover:text-gray-200"
                    }`}
                  >
                    Compact
                  </button>
                  <button
                    type="button"
                    onClick={() => updateSetting("density", "comfortable")}
                    className={`rounded px-2.5 py-0.5 text-[11px] ${
                      settings.density === "comfortable"
                        ? "bg-violet-600 text-white font-medium"
                        : "text-gray-400 hover:text-gray-200"
                    }`}
                  >
                    Comfortable
                  </button>
                </div>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Grid Font Size</div>
                  <div className="text-[10px] text-gray-500">
                    Text size for cells and monospace tables
                  </div>
                </div>
                <div className="flex rounded border border-[var(--border)] bg-[var(--bg)] p-0.5">
                  {(["xs", "sm", "base"] as const).map((size) => (
                    <button
                      key={size}
                      type="button"
                      onClick={() => updateSetting("fontSize", size)}
                      className={`rounded px-2 py-0.5 text-[11px] uppercase ${
                        settings.fontSize === size
                          ? "bg-violet-600 text-white font-medium"
                          : "text-gray-400 hover:text-gray-200"
                      }`}
                    >
                      {size}
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Auto-Refresh Interval</div>
                  <div className="text-[10px] text-gray-500">
                    Seconds between automatic live page reloads
                  </div>
                </div>
                <select
                  value={settings.refreshInterval}
                  onChange={(e) => updateSetting("refreshInterval", Number(e.target.value))}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1 text-gray-200 outline-none focus:border-violet-500"
                >
                  <option value={3}>3 seconds</option>
                  <option value={5}>5 seconds (default)</option>
                  <option value={10}>10 seconds</option>
                  <option value={30}>30 seconds</option>
                  <option value={60}>1 minute</option>
                </select>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">NULL Display Label</div>
                  <div className="text-[10px] text-gray-500">
                    How SQL NULL values appear in data cells
                  </div>
                </div>
                <select
                  value={settings.nullLabel}
                  onChange={(e) => updateSetting("nullLabel", e.target.value)}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1 text-gray-200 outline-none focus:border-violet-500 font-mono"
                >
                  <option value="NULL">NULL (italic)</option>
                  <option value="—">— (dash)</option>
                  <option value="(empty)">(empty)</option>
                  <option value="ø">ø (null symbol)</option>
                </select>
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Show Row Numbers</div>
                  <div className="text-[10px] text-gray-500">
                    Display 1-based index numbering in data tables
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={settings.showRowNumbers}
                  onChange={(e) => updateSetting("showRowNumbers", e.target.checked)}
                  className="h-4 w-4 rounded accent-violet-600"
                />
              </div>

              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Max Export Row Cap</div>
                  <div className="text-[10px] text-gray-500">
                    Safety limit for full table CSV exports
                  </div>
                </div>
                <select
                  value={settings.maxExportLimit}
                  onChange={(e) => updateSetting("maxExportLimit", Number(e.target.value))}
                  className="rounded border border-[var(--border)] bg-[var(--bg)] px-2.5 py-1 text-gray-200 outline-none focus:border-violet-500"
                >
                  <option value={10000}>10,000 rows</option>
                  <option value={50000}>50,000 rows (default)</option>
                  <option value={100000}>100,000 rows</option>
                </select>
              </div>
            </div>
          )}

          {tab === "query" && (
            <div className="space-y-3.5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-gray-200 font-medium">Confirm Destructive Actions</div>
                  <div className="text-[10px] text-gray-500">
                    Show confirmation prompt before DROP TABLE or TRUNCATE
                  </div>
                </div>
                <input
                  type="checkbox"
                  checked={settings.confirmDestructive}
                  onChange={(e) => updateSetting("confirmDestructive", e.target.checked)}
                  className="h-4 w-4 rounded accent-violet-600"
                />
              </div>

              <div className="rounded-lg border border-[var(--border)] bg-[var(--surface-2)] p-3 text-[11px] text-gray-400 space-y-2">
                <div className="font-medium text-gray-300">Keyboard Shortcuts in SQL Console</div>
                <div className="grid grid-cols-2 gap-2 text-[10px]">
                  <div><kbd className="rounded bg-[var(--border)] px-1.5 py-0.5 font-mono text-gray-200">Ctrl + Enter</kbd> Run SQL Query</div>
                  <div><kbd className="rounded bg-[var(--border)] px-1.5 py-0.5 font-mono text-gray-200">Ctrl + Shift + Enter</kbd> Run EXPLAIN</div>
                  <div><kbd className="rounded bg-[var(--border)] px-1.5 py-0.5 font-mono text-gray-200">Ctrl + Wheel</kbd> Horizontal Scroll</div>
                  <div><kbd className="rounded bg-[var(--border)] px-1.5 py-0.5 font-mono text-gray-200">Double Click</kbd> Inspect Row Data</div>
                </div>
              </div>
            </div>
          )}

          {tab === "saved" && (
            <div className="space-y-2.5">
              <div className="text-[11px] text-gray-400">
                Remembered credentials stored in the encrypted OS keychain for this server:
              </div>
              {loadingLogins ? (
                <p className="py-4 text-center text-gray-500">Loading saved credentials…</p>
              ) : savedLogins.length === 0 ? (
                <div className="rounded-lg border border-[var(--border)] p-4 text-center text-gray-500">
                  No saved logins for this server. Check "Remember this password" when signing in to save one.
                </div>
              ) : (
                <div className="space-y-1.5">
                  {savedLogins.map((saved) => (
                    <div
                      key={saved.id}
                      className="flex items-center justify-between rounded-lg border border-[var(--border)] bg-[var(--bg)] p-2.5"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-gray-200">{saved.username}</span>
                          <span className="rounded bg-violet-950/60 px-1.5 py-0.2 text-[9px] font-mono text-violet-300 uppercase">
                            {saved.engine}
                          </span>
                        </div>
                        <div className="mt-0.5 truncate font-mono text-[10px] text-gray-500">
                          {saved.container ? `docker:${saved.container} · ` : ""}
                          {saved.host}:{saved.port}
                          {saved.database ? ` · db:${saved.database}` : ""}
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => void handleForgetLogin(saved.id)}
                        className="rounded p-1.5 text-gray-400 hover:bg-red-950/50 hover:text-red-300 transition-colors"
                        data-tooltip="Forget and delete saved password"
                      >
                        <TrashIcon size={13} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--surface-2)] px-4 py-2.5">
          <div className="flex items-center gap-1.5 text-[11px] text-emerald-400">
            {savedSuccess && (
              <>
                <CheckIcon size={13} />
                <span>Preferences saved</span>
              </>
            )}
          </div>
          <button
            onClick={onClose}
            className="rounded bg-violet-600 px-3.5 py-1 text-[11px] font-medium text-white hover:bg-violet-500 transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

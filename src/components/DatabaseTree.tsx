import { useMemo, useState } from "react";
import {
  api,
  DB_PRODUCT_LABEL,
  type DbEndpoint,
  type DbSavedConnection,
  type DbTable,
} from "../lib/tauri";
import {
  DatabaseIcon,
  RefreshIcon,
  SearchIcon,
  CloseIcon,
  TrashIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  LayersIcon,
} from "./icons";
import { useMaskHost } from "../lib/privacy";

/**
 * One database server found on the host, plus whatever we've learned about it.
 *
 * Every instance is listed as soon as it's discovered, signed in or not, so the tree
 * answers "what is actually running on this box" before any credential is typed. Sign-in
 * is per instance because a native install and a container routinely have different
 * passwords — one shared login form would just fail against half of them.
 */
export interface DbInstance {
  endpoint: DbEndpoint;
  /** A remembered login for this endpoint, if one was saved. */
  saved?: DbSavedConnection;
  /** Backend session id once signed in. */
  sessionId: string | null;
  version: string;
  /** Schema names, loaded on sign-in. */
  schemas: string[];
  /** Tables per schema, loaded lazily when a schema is opened. */
  tables: Record<string, DbTable[]>;
  expanded: boolean;
  openSchemas: string[];
  busy: boolean;
  error: string | null;
}

export function newInstance(endpoint: DbEndpoint): DbInstance {
  return {
    endpoint,
    sessionId: null,
    version: "",
    schemas: [],
    tables: {},
    expanded: false,
    openSchemas: [],
    busy: false,
    error: null,
  };
}

function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Sign-in form for one instance, shown inline under its row. */
function SignIn({
  instance,
  vpsId,
  onConnected,
  onError,
  onSaved,
  onForget,
}: {
  instance: DbInstance;
  /** Which server to tunnel through — the endpoint itself doesn't carry it. */
  vpsId: string;
  onConnected: (sessionId: string, version: string, schemas: string[]) => void;
  onError: (message: string) => void;
  /** Re-read the saved list after one is added. */
  onSaved: () => void;
  onForget: (id: string) => void;
}) {
  const saved = instance.saved;
  const [user, setUser] = useState(
    saved?.username ?? (instance.endpoint.engine === "redis" ? "default" : "root"),
  );
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(Boolean(saved));
  const [busy, setBusy] = useState(false);

  const [showAdvanced, setShowAdvanced] = useState(false);
  const [host, setHost] = useState(saved?.host ?? instance.endpoint.host);
  const [port, setPort] = useState(String(saved?.port ?? instance.endpoint.port));
  const [container, setContainer] = useState(
    saved?.container ?? instance.endpoint.container ?? "",
  );

  const finish = async (sessionId: string, version: string) => {
    setPassword("");
    const schemas = await api.dbListDatabases(sessionId);
    onConnected(sessionId, version, schemas);
  };

  /** Open a remembered login without retyping anything. */
  const useSaved = async () => {
    if (!saved || busy) return;
    setBusy(true);
    try {
      const res = await api.dbConnectSaved(saved.id, vpsId);
      await finish(res.session_id, res.version);
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    try {
      const { endpoint } = instance;
      if (!endpoint.engine) return;
      const parsedPort = Number(port);
      if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
        onError("Port must be a number between 1 and 65535.");
        return;
      }
      const target = {
        vps_id: vpsId,
        container: container.trim() === "" ? null : container.trim(),
        host: host.trim() || endpoint.host,
        port: parsedPort,
        user,
        password,
        database: null,
        engine: endpoint.engine,
      };
      const res = await api.dbConnect(target);
      if (remember) {
        await api.dbSaveConnection(endpoint.id, target);
        onSaved();
      }
      await finish(res.session_id, res.version);
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="space-y-1.5 py-1.5 pl-5 pr-2" onSubmit={submit}>
      {saved?.has_secret ? (
        <div className="flex gap-1">
          <button
            type="button"
            onClick={() => void useSaved()}
            disabled={busy}
            className="min-w-0 flex-1 truncate rounded bg-violet-600 px-2 py-0.5 text-[11px] text-white hover:bg-violet-500 disabled:opacity-50"
          >
            {busy ? "Connecting…" : `Connect as ${saved.username}`}
          </button>
          <button
            type="button"
            onClick={() => void onForget(saved.id)}
            className="shrink-0 rounded border border-[var(--border)] p-1 text-[11px] text-gray-400 hover:text-red-300"
            data-tooltip="Forget this saved password"
          >
            <TrashIcon size={12} />
          </button>
        </div>
      ) : null}

      <div className="flex gap-1">
        <input
          value={user}
          onChange={(e) => setUser(e.target.value)}
          placeholder="user"
          className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
        />
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="password"
          className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
        />
      </div>

      <button
        type="button"
        onClick={() => setShowAdvanced((v) => !v)}
        className="flex items-center gap-1 text-left text-[10px] text-gray-500 hover:text-gray-300"
      >
        {showAdvanced ? <ChevronDownIcon size={10} /> : <ChevronRightIcon size={10} />}
        <span>Connection details</span>
        {!showAdvanced ? (
          <span className="ml-1 font-mono text-gray-600">
            {container ? `${container}:` : ""}
            {host}:{port}
          </span>
        ) : null}
      </button>

      {showAdvanced ? (
        <div className="space-y-1 rounded border border-[var(--border)] p-1.5 bg-[var(--bg)]">
          <div className="flex gap-1">
            <input
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="host"
              className="min-w-0 flex-[2] rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
            />
            <input
              value={port}
              onChange={(e) => setPort(e.target.value)}
              placeholder="port"
              inputMode="numeric"
              className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
            />
          </div>
          <input
            value={container}
            onChange={(e) => setContainer(e.target.value)}
            placeholder="container (blank = run on host)"
            className="w-full rounded border border-[var(--border)] bg-[var(--surface)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
          />
        </div>
      ) : null}

      <label className="flex items-center gap-1.5 text-[10px] text-gray-500">
        <input
          type="checkbox"
          checked={remember}
          onChange={(e) => setRemember(e.target.checked)}
          className="accent-violet-600 rounded"
        />
        Remember this password
      </label>

      <button
        type="submit"
        disabled={busy}
        className="w-full rounded bg-violet-600 px-2 py-0.5 text-[11px] font-medium text-white hover:bg-violet-500 disabled:opacity-50"
      >
        {busy ? "Connecting…" : "Sign in"}
      </button>
    </form>
  );
}

/**
 * The whole server's databases in one tree: every database instance found on the host —
 * native installs and Docker containers alike, named and labelled with what they are —
 * and under each, its schemas/databases and tables.
 */
export function DatabaseTree({
  instances,
  vpsId,
  scanning,
  selected,
  position = "left",
  width = 224,
  height = 160,
  onPatch,
  onSelectTable,
  onRescan,
  onSavedChanged,
  onForget,
  onDisconnect,
}: {
  instances: DbInstance[];
  vpsId: string;
  scanning: boolean;
  selected: { endpointId: string; schema: string; table: string } | null;
  position?: "left" | "right" | "top" | "bottom";
  width?: number;
  height?: number;
  onPatch: (endpointId: string, patch: Partial<DbInstance>) => void;
  onSelectTable: (instance: DbInstance, schema: string, table: string) => void;
  onRescan: () => void;
  onSavedChanged: () => void;
  onForget: (id: string) => void;
  onDisconnect?: (instance: DbInstance) => void;
}) {
  const maskHost = useMaskHost();
  const [filter, setFilter] = useState("");
  const isHorizontal = position === "top" || position === "bottom";
  const [connectedOnly, setConnectedOnly] = useState(() => {
    return localStorage.getItem("xconsole-db-view-mode") === "connected";
  });

  const setConnectedOnlyPersist = (val: boolean) => {
    setConnectedOnly(val);
    try {
      localStorage.setItem("xconsole-db-view-mode", val ? "connected" : "all");
    } catch {
      /* ignore */
    }
  };

  const filterQ = filter.trim().toLowerCase();

  const toggleInstance = (inst: DbInstance) =>
    onPatch(inst.endpoint.id, { expanded: !inst.expanded });

  const toggleSchema = async (inst: DbInstance, schema: string, forceReload = false) => {
    const open = inst.openSchemas.includes(schema);
    if (open && !forceReload) {
      onPatch(inst.endpoint.id, {
        openSchemas: inst.openSchemas.filter((s) => s !== schema),
      });
      return;
    }
    if (!open) {
      onPatch(inst.endpoint.id, { openSchemas: [...inst.openSchemas, schema] });
    }
    if ((inst.tables[schema] && !forceReload) || !inst.sessionId) return;
    onPatch(inst.endpoint.id, { busy: true });
    try {
      const tables = await api.dbListTables(inst.sessionId, schema);
      onPatch(inst.endpoint.id, {
        tables: { ...inst.tables, [schema]: tables },
        busy: false,
        error: null,
      });
    } catch (e) {
      onPatch(inst.endpoint.id, { busy: false, error: String(e) });
    }
  };

  const connectedCount = instances.filter((i) => i.sessionId).length;

  const visibleInstances = useMemo(() => {
    if (connectedOnly && connectedCount > 0) {
      return instances.filter((i) => i.sessionId);
    }
    return instances;
  }, [instances, connectedOnly, connectedCount]);

  // Count total matches when filtering
  const matchCount = useMemo(() => {
    if (!filterQ) return 0;
    let count = 0;
    for (const inst of visibleInstances) {
      for (const [schema, tbls] of Object.entries(inst.tables)) {
        if (schema.toLowerCase().includes(filterQ)) count += 1;
        for (const t of tbls) {
          if (t.name.toLowerCase().includes(filterQ)) count += 1;
        }
      }
    }
    return count;
  }, [visibleInstances, filterQ]);

  const containerStyle = isHorizontal
    ? { height: `${height}px`, width: "100%" }
    : { width: `${width}px`, height: "100%" };

  const borderClass =
    position === "left"
      ? "border-r"
      : position === "right"
        ? "border-l"
        : position === "top"
          ? "border-b"
          : "border-t";

  return (
    <div
      style={containerStyle}
      className={`flex shrink-0 flex-col ${borderClass} border-[var(--border)] bg-[var(--surface-2)] select-none overflow-hidden transition-all duration-75`}
    >
      {/* Top Header */}
      <div className="flex shrink-0 items-center justify-between gap-1 border-b border-[var(--border)] px-2 py-1.5 bg-[var(--surface)]">
        <span className="text-[10px] uppercase font-semibold tracking-wider text-gray-400">
          Databases
        </span>
        <div className="flex items-center gap-0.5">
          {connectedCount > 0 && (
            <button
              onClick={() => setConnectedOnlyPersist(!connectedOnly)}
              className={`rounded p-1 text-[10px] transition-colors ${
                connectedOnly
                  ? "bg-violet-600/30 text-violet-300 font-medium"
                  : "text-gray-500 hover:bg-[var(--border)] hover:text-gray-300"
              }`}
              data-tooltip={
                connectedOnly
                  ? "Showing connected servers only — click to show all"
                  : "Filter to connected servers only"
              }
            >
              <LayersIcon size={12} />
            </button>
          )}
          <button
            onClick={onRescan}
            disabled={scanning}
            className="rounded p-1 text-gray-500 hover:bg-[var(--border)] hover:text-white disabled:opacity-40"
            data-tooltip="Rescan for databases on this host"
          >
            <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {/* Fast Search Input */}
      <div className="shrink-0 border-b border-[var(--border)] px-1.5 py-1 bg-[var(--bg)]">
        <div className="relative flex items-center">
          <SearchIcon
            size={11}
            className="absolute left-1.5 text-gray-500 pointer-events-none"
          />
          <input
            type="text"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setFilter("");
            }}
            placeholder="Quick search tables…"
            spellCheck={false}
            className="w-full rounded border border-[var(--border)] bg-[var(--surface)] pl-5 pr-5 py-0.5 text-[10px] text-gray-200 outline-none focus:border-violet-500 placeholder:text-gray-600"
          />
          {filter && (
            <button
              type="button"
              onClick={() => setFilter("")}
              className="absolute right-1 text-gray-400 hover:text-white p-0.5"
              data-tooltip="Clear search (Esc)"
            >
              <CloseIcon size={10} />
            </button>
          )}
        </div>
        {filterQ && (
          <div className="mt-0.5 px-0.5 text-[9px] text-violet-400 font-mono">
            {matchCount} match{matchCount === 1 ? "" : "es"} in loaded tables
          </div>
        )}
      </div>

      {/* Database Tree Items */}
      <div className="min-h-0 flex-1 overflow-auto py-1 space-y-0.5">
        {scanning && instances.length === 0 ? (
          <p className="px-2 py-2 text-[11px] text-gray-500">
            Looking for databases over SSH…
          </p>
        ) : null}

        {!scanning && visibleInstances.length === 0 ? (
          <p className="px-2 py-2 text-[11px] text-gray-500">
            {connectedOnly
              ? "No connected databases. Switch view or sign in below."
              : "No databases found on this host."}
          </p>
        ) : null}

        {visibleInstances.map((inst) => {
          const { endpoint: ep } = inst;
          const docker = ep.kind === "docker";
          return (
            <div key={ep.id} className="group">
              <div className="flex w-full items-center gap-1 px-2 py-0.5 text-left text-[11px] text-gray-200 hover:bg-[var(--border)]/70">
                <button
                  type="button"
                  onClick={() => toggleInstance(inst)}
                  className="flex min-w-0 flex-1 items-center gap-1 text-left"
                  title={`${maskHost(ep.host)}:${ep.port}${ep.image ? ` · ${ep.image}` : ""}`}
                >
                  <span className="w-2.5 shrink-0 text-gray-500">
                    {inst.expanded ? (
                      <ChevronDownIcon size={10} />
                    ) : (
                      <ChevronRightIcon size={10} />
                    )}
                  </span>
                  <span
                    className="shrink-0 text-xs"
                    title={docker ? "Docker container" : "Installed on the host"}
                  >
                    {docker ? "🐳" : "🖥"}
                  </span>
                  <span className="truncate font-medium text-[11px]">
                    {docker && ep.container ? ep.container : "host"}
                  </span>
                  <span className="shrink-0 rounded bg-[var(--border)] px-1 py-0.2 text-[8px] font-mono text-gray-400">
                    {DB_PRODUCT_LABEL[ep.product] ?? ep.product}
                  </span>
                  <span className="ml-auto shrink-0 font-mono text-[9px] text-gray-600">
                    :{ep.port}
                  </span>
                  {inst.sessionId ? (
                    <span
                      className="shrink-0 h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse"
                      title="Connected"
                    />
                  ) : null}
                </button>

                {inst.sessionId && onDisconnect && (
                  <button
                    type="button"
                    onClick={() => onDisconnect(inst)}
                    className="opacity-0 group-hover:opacity-100 rounded p-0.5 text-gray-500 hover:text-red-300 transition-opacity"
                    data-tooltip="Disconnect session"
                  >
                    <CloseIcon size={10} />
                  </button>
                )}
              </div>

              {inst.expanded ? (
                <>
                  {inst.error ? (
                    <p className="px-2 py-0.5 pl-5 text-[10px] text-red-400 break-words">
                      {inst.error}
                    </p>
                  ) : null}

                  {!ep.engine ? (
                    <p className="px-2 py-1 pl-5 text-[10px] text-gray-500">
                      Found, but browsing {DB_PRODUCT_LABEL[ep.product] ?? ep.product} isn't
                      supported yet. Use a terminal on this server.
                    </p>
                  ) : !inst.sessionId ? (
                    <SignIn
                      instance={inst}
                      vpsId={vpsId}
                      onConnected={(sessionId, version, schemas) =>
                        onPatch(ep.id, { sessionId, version, schemas, error: null })
                      }
                      onError={(message) => onPatch(ep.id, { error: message })}
                      onSaved={onSavedChanged}
                      onForget={onForget}
                    />
                  ) : (
                    <>
                      {inst.version ? (
                        <p className="px-2 pl-5 text-[9px] font-mono text-gray-500 truncate" title={inst.version}>
                          {inst.version}
                        </p>
                      ) : null}
                      {inst.schemas.length === 0 ? (
                        <p className="px-2 py-0.5 pl-5 text-[10px] text-gray-600">
                          No databases visible to this user.
                        </p>
                      ) : null}
                      {inst.schemas.map((schema) => {
                        // Auto open when filtering
                        const open =
                          inst.openSchemas.includes(schema) ||
                          Boolean(filterQ && schema.toLowerCase().includes(filterQ));
                        const rawTables = inst.tables[schema] ?? [];
                        const tables = filterQ
                          ? rawTables.filter(
                              (t) =>
                                t.name.toLowerCase().includes(filterQ) ||
                                schema.toLowerCase().includes(filterQ),
                            )
                          : rawTables;

                        return (
                          <div key={schema} className="group/schema">
                            <div className="flex w-full items-center gap-1 py-0.5 pl-5 pr-2 text-left text-[11px] text-gray-300 hover:bg-[var(--border)]/50">
                              <button
                                type="button"
                                onClick={() => void toggleSchema(inst, schema)}
                                className="flex min-w-0 flex-1 items-center gap-1 text-left"
                              >
                                <span className="w-2.5 shrink-0 text-gray-500">
                                  {open ? (
                                    <ChevronDownIcon size={10} />
                                  ) : (
                                    <ChevronRightIcon size={10} />
                                  )}
                                </span>
                                <DatabaseIcon size={11} className="shrink-0 text-violet-400" />
                                <span className="truncate font-medium text-[11px]">{schema}</span>
                              </button>
                              <button
                                type="button"
                                onClick={() => void toggleSchema(inst, schema, true)}
                                className="opacity-0 group-hover/schema:opacity-100 rounded p-0.5 text-gray-500 hover:text-gray-200 transition-opacity"
                                data-tooltip={`Refresh ${schema} tables`}
                              >
                                <RefreshIcon size={9} />
                              </button>
                            </div>

                            {open
                              ? tables.map((t) => {
                                  const active =
                                    selected?.endpointId === ep.id &&
                                    selected?.schema === schema &&
                                    selected?.table === t.name;
                                  return (
                                    <button
                                      key={t.name}
                                      onClick={() => onSelectTable(inst, schema, t.name)}
                                      className={`flex w-full items-center gap-1 truncate py-0.5 pl-10 pr-2 text-left text-[11px] transition-colors ${
                                        active
                                          ? "bg-violet-600/25 text-violet-200 font-medium border-l-2 border-violet-500"
                                          : "text-gray-400 hover:bg-[var(--border)]/60 hover:text-gray-200"
                                      }`}
                                      title={`${t.rows.toLocaleString()} rows · ${bytes(t.bytes)} · ${t.engine || t.kind}`}
                                    >
                                      <span className="min-w-0 flex-1 truncate font-mono text-[10.5px]">
                                        {t.name}
                                      </span>
                                      {t.rows > 0 ? (
                                        <span className="shrink-0 tabular-nums font-mono text-[9px] text-gray-500">
                                          {t.rows >= 1_000_000
                                            ? `${(t.rows / 1_000_000).toFixed(1)}M`
                                            : t.rows >= 1000
                                              ? `${(t.rows / 1000).toFixed(t.rows >= 10_000 ? 0 : 1)}k`
                                              : t.rows}
                                        </span>
                                      ) : null}
                                    </button>
                                  );
                                })
                              : null}

                            {open && tables.length === 0 ? (
                              <p className="py-0.5 pl-10 text-[10px] text-gray-600">
                                {inst.busy
                                  ? "Loading tables…"
                                  : filterQ
                                    ? "No matching tables."
                                    : "No tables in database."}
                              </p>
                            ) : null}
                          </div>
                        );
                      })}
                    </>
                  )}
                </>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}


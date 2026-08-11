import { useState } from "react";
import {
  api,
  DB_PRODUCT_LABEL,
  type DbEndpoint,
  type DbSavedConnection,
  type DbTable,
} from "../lib/tauri";
import { DatabaseIcon } from "./icons";

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

  // Detection is a good guess, not gospel — a database can listen somewhere the scan
  // can't see, or be attributed to the wrong container. These start from what was
  // detected (or last saved) and can be corrected without leaving the panel.
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
      if (!endpoint.engine) return; // guarded by the caller; keeps the type honest
      const parsedPort = Number(port);
      if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
        onError("Port must be a number between 1 and 65535.");
        return;
      }
      const target = {
        vps_id: vpsId,
        // Empty means "run on the host", not "run in a container called ''".
        container: container.trim() === "" ? null : container.trim(),
        host: host.trim() || endpoint.host,
        port: parsedPort,
        user,
        password,
        database: null,
        engine: endpoint.engine,
      };
      const res = await api.dbConnect(target);
      // Save only after the credentials are known good, so a typo isn't remembered.
      if (remember) {
        await api.dbSaveConnection(endpoint.id, target);
        onSaved();
      }
      // Drop it from component state immediately — the backend holds it for the session.
      await finish(res.session_id, res.version);
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="space-y-1 py-1 pl-5 pr-2" onSubmit={submit}>
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
            className="shrink-0 rounded border border-[var(--border)] px-1.5 text-[11px] text-gray-400 hover:text-red-300"
            data-tooltip="Forget this saved password"
          >
            ✕
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
        className="text-left text-[10px] text-gray-500 hover:text-gray-300"
      >
        {showAdvanced ? "▾" : "▸"} Connection details
        {!showAdvanced ? (
          <span className="ml-1 font-mono text-gray-600">
            {container ? `${container}:` : ""}
            {host}:{port}
          </span>
        ) : null}
      </button>

      {showAdvanced ? (
        <div className="space-y-1 rounded border border-[var(--border)] p-1">
          <div className="flex gap-1">
            <input
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="host"
              className="min-w-0 flex-[2] rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
            />
            <input
              value={port}
              onChange={(e) => setPort(e.target.value)}
              placeholder="port"
              inputMode="numeric"
              className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
            />
          </div>
          <input
            value={container}
            onChange={(e) => setContainer(e.target.value)}
            placeholder="container (blank = run on the host)"
            className="w-full rounded border border-[var(--border)] bg-[var(--bg)] px-1.5 py-0.5 text-[11px] text-gray-100 outline-none focus:border-violet-500"
          />
          <p className="text-[10px] leading-snug text-gray-600">
            The host and port are as seen <em>from the server</em>, not from this PC —
            everything runs over SSH. Set a container to run the client inside it, which
            is what you need when the database's client isn't installed on the host.
          </p>
        </div>
      ) : null}

      <label className="flex items-center gap-1.5 text-[10px] text-gray-500">
        <input
          type="checkbox"
          checked={remember}
          onChange={(e) => setRemember(e.target.checked)}
        />
        Remember this password
      </label>

      <button
        type="submit"
        disabled={busy}
        className="w-full rounded bg-violet-600 px-2 py-0.5 text-[11px] text-white hover:bg-violet-500 disabled:opacity-50"
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
 *
 * Products the client can't drive yet are still listed, marked as such. Dropping them
 * would recreate the original bug, where a running Postgres container simply didn't
 * appear and there was no way to tell whether it had been missed or wasn't there.
 */
export function DatabaseTree({
  instances,
  vpsId,
  scanning,
  selected,
  onPatch,
  onSelectTable,
  onRescan,
  onSavedChanged,
  onForget,
}: {
  instances: DbInstance[];
  vpsId: string;
  scanning: boolean;
  selected: { endpointId: string; schema: string; table: string } | null;
  onPatch: (endpointId: string, patch: Partial<DbInstance>) => void;
  onSelectTable: (instance: DbInstance, schema: string, table: string) => void;
  onRescan: () => void;
  onSavedChanged: () => void;
  onForget: (id: string) => void;
}) {
  const toggleInstance = (inst: DbInstance) =>
    onPatch(inst.endpoint.id, { expanded: !inst.expanded });

  const toggleSchema = async (inst: DbInstance, schema: string) => {
    const open = inst.openSchemas.includes(schema);
    if (open) {
      onPatch(inst.endpoint.id, {
        openSchemas: inst.openSchemas.filter((s) => s !== schema),
      });
      return;
    }
    onPatch(inst.endpoint.id, { openSchemas: [...inst.openSchemas, schema] });
    if (inst.tables[schema] || !inst.sessionId) return;
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

  return (
    <div className="flex h-full w-56 shrink-0 flex-col border-r border-[var(--border)] bg-[var(--surface-2)]">
      <div className="flex shrink-0 items-center gap-1 border-b border-[var(--border)] px-2 py-1">
        <span className="text-[10px] uppercase tracking-wider text-gray-500">
          Databases on this server
        </span>
        <button
          onClick={onRescan}
          disabled={scanning}
          className="ml-auto rounded px-1 text-[10px] text-gray-500 hover:bg-[var(--border)] hover:text-white disabled:opacity-40"
          data-tooltip="Scan again"
        >
          ⟳
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto py-1">
        {scanning && instances.length === 0 ? (
          <p className="px-2 py-2 text-[11px] text-gray-500">
            Looking for databases over SSH…
          </p>
        ) : null}

        {!scanning && instances.length === 0 ? (
          <p className="px-2 py-2 text-[11px] text-gray-500">
            No databases found — nothing listening on a known port, and no matching
            container running.
          </p>
        ) : null}

        {instances.map((inst) => {
          const { endpoint: ep } = inst;
          const docker = ep.kind === "docker";
          return (
            <div key={ep.id}>
              <button
                onClick={() => toggleInstance(inst)}
                className="flex w-full items-center gap-1 px-2 py-0.5 text-left text-[11px] text-gray-200 hover:bg-[var(--border)]"
                title={`${ep.host}:${ep.port}${ep.image ? ` · ${ep.image}` : ""}`}
              >
                <span className="w-2 shrink-0 text-gray-600">
                  {inst.expanded ? "▾" : "▸"}
                </span>
                <span className="shrink-0" title={docker ? "Docker container" : "Installed on the host"}>
                  {docker ? "🐳" : "🖥"}
                </span>
                <span className="truncate">
                  {docker && ep.container ? ep.container : "host"}
                </span>
                <span className="shrink-0 rounded bg-[var(--border)] px-1 text-[9px] text-gray-400">
                  {DB_PRODUCT_LABEL[ep.product] ?? ep.product}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-gray-600">
                  :{ep.port}
                </span>
                {inst.sessionId ? (
                  <span className="shrink-0 text-[9px] text-green-500">●</span>
                ) : null}
              </button>

              {inst.expanded ? (
                <>
                  {inst.error ? (
                    <p className="px-2 py-0.5 pl-5 text-[10px] text-red-400">{inst.error}</p>
                  ) : null}

                  {!ep.engine ? (
                    // Discovered but not yet openable. Saying so beats a sign-in form
                    // that would fail, and beats hiding the instance entirely — knowing
                    // it is there is most of the value.
                    <p className="px-2 py-1 pl-5 text-[10px] text-gray-500">
                      Found, but browsing {DB_PRODUCT_LABEL[ep.product] ?? ep.product} isn't
                      supported yet. Use a terminal on this server to reach it.
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
                        <p className="px-2 pl-5 text-[10px] text-gray-600">{inst.version}</p>
                      ) : null}
                      {inst.schemas.length === 0 ? (
                        <p className="px-2 py-0.5 pl-5 text-[10px] text-gray-600">
                          No databases visible to this user.
                        </p>
                      ) : null}
                      {inst.schemas.map((schema) => {
                        const open = inst.openSchemas.includes(schema);
                        return (
                          <div key={schema}>
                            <button
                              onClick={() => void toggleSchema(inst, schema)}
                              className="flex w-full items-center gap-1 py-0.5 pl-5 pr-2 text-left text-[11px] text-gray-300 hover:bg-[var(--border)]"
                            >
                              <span className="w-2 shrink-0 text-gray-600">
                                {open ? "▾" : "▸"}
                              </span>
                              <DatabaseIcon size={11} className="shrink-0 text-violet-400/70" />
                              <span className="truncate">{schema}</span>
                            </button>
                            {open
                              ? (inst.tables[schema] ?? []).map((t) => {
                                  const active =
                                    selected?.endpointId === ep.id &&
                                    selected?.schema === schema &&
                                    selected?.table === t.name;
                                  return (
                                    <button
                                      key={t.name}
                                      onClick={() => onSelectTable(inst, schema, t.name)}
                                      className={`flex w-full items-center gap-1 truncate py-0.5 pl-12 pr-2 text-left text-[11px] ${
                                        active
                                          ? "bg-violet-600/25 text-violet-200"
                                          : "text-gray-400 hover:bg-[var(--border)]"
                                      }`}
                                      title={`${t.rows.toLocaleString()} rows · ${bytes(t.bytes)} · ${t.engine || t.kind}`}
                                    >
                                      <span className="min-w-0 flex-1 truncate">{t.name}</span>
                                      {t.rows > 0 ? (
                                        <span className="shrink-0 tabular-nums text-[9px] text-gray-600">
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
                            {open && (inst.tables[schema] ?? []).length === 0 ? (
                              <p className="py-0.5 pl-12 text-[10px] text-gray-600">
                                {inst.busy ? "Loading…" : "No tables."}
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

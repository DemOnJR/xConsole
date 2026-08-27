import { invoke } from "@tauri-apps/api/core";
import {
  DatabaseIcon,
  RefreshIcon,
  ChevronDownIcon,
  ChevronRightIcon,
} from "./icons";

export interface DbEndpoint {
  id: string;
  vps_id: string;
  product: string;
  kind: "native" | "docker";
  engine: string | null;
  host: string;
  port: number;
  container?: string | null;
  image?: string | null;
}

export interface DbTable {
  name: string;
  rows: number;
  bytes: number;
  engine?: string;
  kind?: string;
}

export interface DbInstance {
  endpoint: DbEndpoint;
  sessionId: string | null;
  version: string;
  schemas: string[];
  tables: Record<string, DbTable[]>;
  expanded: boolean;
  openSchemas: string[];
  busy: boolean;
  error: string | null;
}

export function DatabaseTree({
  instances,
  scanning,
  onPatch,
  onSelectTable,
  onRescan,
}: {
  instances: DbInstance[];
  scanning: boolean;
  onPatch: (endpointId: string, patch: Partial<DbInstance>) => void;
  onSelectTable: (instance: DbInstance, schema: string, table: string) => void;
  onRescan: () => void;
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
      const tables = await invoke<DbTable[]>("db_list_tables", { sessionId: inst.sessionId, schema });
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
    <div className="flex h-full w-60 shrink-0 flex-col border-r border-[var(--border)] bg-[var(--surface-2)] select-none overflow-hidden">
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--border)] px-2.5 py-2 bg-[var(--surface)]">
        <span className="text-[11px] uppercase font-semibold tracking-wider text-gray-400">
          Databases ({instances.length})
        </span>
        <button
          onClick={onRescan}
          disabled={scanning}
          className="rounded p-1 text-gray-400 hover:text-white"
          title="Rescan databases"
        >
          <RefreshIcon size={12} className={scanning ? "animate-spin" : ""} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto py-1 space-y-0.5">
        {instances.map((inst) => {
          const { endpoint: ep } = inst;
          return (
            <div key={ep.id} className="group">
              <div className="flex w-full items-center gap-1 px-2 py-1 text-left text-xs text-gray-200 hover:bg-[var(--border)]/70 cursor-pointer" onClick={() => toggleInstance(inst)}>
                <span className="w-3 text-gray-500">
                  {inst.expanded ? <ChevronDownIcon size={10} /> : <ChevronRightIcon size={10} />}
                </span>
                <DatabaseIcon size={12} className="text-violet-400" />
                <span className="truncate font-medium">{ep.container || ep.host}</span>
                <span className="ml-auto font-mono text-[10px] text-gray-500">:{ep.port}</span>
              </div>

              {inst.expanded && (
                <div className="pl-5 pr-2 py-1 space-y-1">
                  {inst.schemas.map((schema) => (
                    <div key={schema}>
                      <div
                        className="flex items-center gap-1 py-0.5 text-xs text-gray-300 hover:text-white cursor-pointer"
                        onClick={() => toggleSchema(inst, schema)}
                      >
                        <span>{inst.openSchemas.includes(schema) ? "▾" : "▸"}</span>
                        <span>{schema}</span>
                      </div>

                      {inst.openSchemas.includes(schema) && (
                        <div className="pl-4 space-y-0.5">
                          {(inst.tables[schema] || []).map((tbl) => (
                            <div
                              key={tbl.name}
                              className="text-[11px] text-gray-400 hover:text-violet-300 cursor-pointer truncate font-mono py-0.2"
                              onClick={() => onSelectTable(inst, schema, tbl.name)}
                            >
                              {tbl.name} ({tbl.rows})
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

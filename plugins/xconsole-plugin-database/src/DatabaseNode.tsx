import { useState } from "react";
import { DatabaseTree, type DbInstance } from "./DatabaseTree";
import { DatabaseIcon } from "./icons";

export function DatabaseNode() {
  const [instances, setInstances] = useState<DbInstance[]>([]);
  const [selectedTable, setSelectedTable] = useState<string | null>(null);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-[var(--border)] bg-[var(--surface)] shadow-lg font-mono">
      <div className="flex items-center justify-between border-b border-[var(--border)] bg-[var(--surface-2)] px-3 py-2 text-xs">
        <div className="flex items-center gap-2">
          <DatabaseIcon size={14} className="text-violet-400" />
          <span className="font-semibold text-gray-200">Database Inspector (MySQL / Postgres / Redis)</span>
        </div>
      </div>

      <div className="flex flex-1 min-h-0 overflow-hidden">
        <DatabaseTree
          instances={instances}
          scanning={false}
          onPatch={(epId, patch) => {
            setInstances((cur) =>
              cur.map((i) => (i.endpoint.id === epId ? { ...i, ...patch } : i))
            );
          }}
          onSelectTable={(_inst, _schema, table) => setSelectedTable(table)}
          onRescan={() => {}}
        />

        <div className="flex-1 p-4 text-xs text-gray-400 overflow-auto">
          {selectedTable ? (
            <div>
              <h3 className="text-sm font-semibold text-gray-200 mb-2">Table: {selectedTable}</h3>
              <p>Ready to execute SQL queries or inspect rows.</p>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-gray-500">
              Select a database and table from the sidebar to inspect schema &amp; data.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

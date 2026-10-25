import { Copy, Plus } from "lucide-react";
import { useState } from "react";
import type { TableInfo } from "@/bindings/TableInfo";
import { Button } from "@/components/ui/Button";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Input } from "@/components/ui/Input";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { Spinner } from "@/components/ui/Spinner";
import { copyText, type StructurePart, type Tab, useApp } from "@/store/app";
import { ColumnsTab } from "./ColumnsTab";
import { ForeignKeysTab } from "./ForeignKeysTab";
import { IndexesTab } from "./IndexesTab";
import type { PartProps } from "./StructureTable";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/**
 * The table a browse tab shows, one part at a time: columns, indexes, foreign keys or DDL, each
 * with its count and a filter. Columns and indexes can be added, changed and dropped when the
 * driver supports it and the connection is writable; every change is shown as SQL before it runs.
 */
export function StructureView({ tab }: { tab: Tab }) {
  const structure = tab.structure;
  const canChange = useApp((s) => {
    const config = s.connections.find((c) => c.id === tab.connId);
    const status = s.nodes[tab.connId]?.status;
    return status?.kind === "connected" && status.capabilities.schema_edit && !config?.read_only;
  });
  if (!structure || structure.loading) {
    return (
      <div className="animate-fade flex h-full items-center justify-center">
        <Spinner size={18} />
      </div>
    );
  }
  if (structure.error || !structure.info) {
    return (
      <div className="p-4">
        <ErrorNotice message={structure.error ?? "No structure available."} />
      </div>
    );
  }
  return (
    <Structure
      tab={tab.id}
      info={structure.info}
      conn={tab.connId}
      part={tab.structurePart}
      focus={tab.structureFocus}
      canEdit={canChange && !structure.info.view}
    />
  );
}

function Structure({
  tab,
  info,
  conn,
  part,
  focus,
  canEdit,
}: {
  tab: number;
  info: TableInfo;
  conn: string;
  part: StructurePart;
  focus: string | null;
  canEdit: boolean;
}) {
  const { openSchemaForm, setStructurePart } = useApp.getState();
  const [filter, setFilter] = useState("");
  const table = info.table;
  const props: PartProps = { info, conn, needle: filter.trim().toLowerCase(), canEdit, focus };

  const tabs: Array<{ value: StructurePart; label: string }> = [
    { value: "columns", label: `Columns ${info.columns.length}` },
    { value: "indexes", label: `Indexes ${info.indexes.length}` },
    { value: "foreign_keys", label: `Foreign keys ${info.foreign_keys.length}` },
    ...(info.ddl ? [{ value: "ddl" as const, label: "DDL" }] : []),
  ];
  const addLabel = part === "columns" ? "Add column…" : part === "indexes" ? "Add index…" : null;
  const add = () =>
    part === "columns"
      ? openSchemaForm({ kind: "column", conn, table, column: null })
      : openSchemaForm({ kind: "index", conn, table, columns: [] });

  return (
    <div className="animate-fade flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-col gap-2 border-line border-b px-3 py-2">
        <SegmentedTabs
          label="Structure"
          value={part}
          onChange={(value) => {
            setFilter("");
            setStructurePart(tab, value);
          }}
          tabs={tabs}
        />
        {part === "ddl" ? (
          <div className="flex justify-end">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => info.ddl && copyText(info.ddl, "Copied DDL")}
            >
              <Copy {...ICON} />
              Copy
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Input
              aria-label="Filter"
              placeholder="Filter"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              className="min-w-0 flex-1"
            />
            {canEdit && addLabel && (
              <Button size="sm" variant="ghost" onClick={add} className="shrink-0">
                <Plus {...ICON} />
                {addLabel}
              </Button>
            )}
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {part === "columns" && <ColumnsTab {...props} />}
        {part === "indexes" && <IndexesTab {...props} />}
        {part === "foreign_keys" && <ForeignKeysTab {...props} />}
        {part === "ddl" && (
          <pre className="select-text p-3 font-mono text-sm text-fg">{info.ddl}</pre>
        )}
      </div>
    </div>
  );
}

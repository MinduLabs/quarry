import { Pencil, Trash2 } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { useApp } from "@/store/app";
import {
  Cell,
  matchesNeedle,
  NameCell,
  type PartProps,
  Row,
  RowActions,
  Table,
} from "./StructureTable";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

export function ColumnsTab({ info, conn, needle, canEdit }: PartProps) {
  const { openSchemaForm, alterSchema } = useApp.getState();
  const keyColumns = new Set(info.primary_key);
  const table = info.table;
  return (
    <Table head={["Name", "Type", "Null", "Default", "Extra", "Comment"]} actions={canEdit}>
      {info.columns
        .filter((c) => matchesNeedle(needle, c.name, c.type_name, c.comment))
        .map((c) => (
          <Row key={c.name}>
            <NameCell icon={keyColumns.has(c.name) ? "primary_key" : undefined} name={c.name} />
            <Cell mono>{c.type_name}</Cell>
            <Cell>{c.nullable ? "Yes" : "No"}</Cell>
            <Cell mono faint={c.default === null}>
              {c.default ?? "NULL"}
            </Cell>
            <Cell mono>{c.extra ?? ""}</Cell>
            <Cell>{c.comment ?? ""}</Cell>
            {canEdit && (
              <RowActions>
                <IconButton
                  label={c.generated ? "Generated columns are edited in SQL" : "Edit column…"}
                  size="sm"
                  disabled={c.generated}
                  onClick={() => openSchemaForm({ kind: "column", conn, table, column: c })}
                >
                  <Pencil {...ICON} />
                </IconButton>
                <IconButton
                  label="Drop column…"
                  size="sm"
                  disabled={info.columns.length === 1}
                  onClick={() =>
                    alterSchema(conn, {
                      kind: "alter_table",
                      table,
                      ops: [{ op: "drop_column", name: c.name }],
                    })
                  }
                >
                  <Trash2 {...ICON} />
                </IconButton>
              </RowActions>
            )}
          </Row>
        ))}
    </Table>
  );
}

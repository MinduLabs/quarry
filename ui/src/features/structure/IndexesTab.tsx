import { Trash2 } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { useApp } from "@/store/app";
import {
  Cell,
  Empty,
  matchesNeedle,
  NameCell,
  type PartProps,
  Row,
  RowActions,
  Table,
} from "./StructureTable";

/** The primary key first, then the other indexes in the order the driver lists them. */
export function IndexesTab({ info, conn, needle, canEdit, focus }: PartProps) {
  const { alterSchema } = useApp.getState();
  if (info.indexes.length === 0) return <Empty>No indexes</Empty>;
  const indexes = info.indexes
    .filter((i) => matchesNeedle(needle, i.name, i.columns.join(" ")))
    .sort((a, b) => Number(b.primary) - Number(a.primary));
  return (
    <Table head={["Name", "Columns", "Unique"]} actions={canEdit}>
      {indexes.map((i) => (
        <Row key={i.name} focused={focus === i.name}>
          <NameCell icon={i.primary ? "primary_key" : "index"} name={i.name} />
          <Cell mono>{i.columns.join(", ")}</Cell>
          <Cell>{i.unique || i.primary ? "Yes" : "No"}</Cell>
          {canEdit && (
            <RowActions>
              <IconButton
                label={i.primary ? "Drop primary key…" : "Drop index…"}
                size="sm"
                onClick={() =>
                  alterSchema(conn, {
                    kind: "alter_table",
                    table: info.table,
                    ops: [{ op: "drop_index", name: i.name }],
                  })
                }
              >
                <Trash2 size={14} strokeWidth={1.75} />
              </IconButton>
            </RowActions>
          )}
        </Row>
      ))}
    </Table>
  );
}

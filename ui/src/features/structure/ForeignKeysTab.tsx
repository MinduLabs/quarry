import { Cell, Empty, matchesNeedle, NameCell, type PartProps, Row, Table } from "./StructureTable";

export function ForeignKeysTab({ info, needle, focus }: PartProps) {
  if (info.foreign_keys.length === 0) return <Empty>No foreign keys</Empty>;
  const foreignKeys = info.foreign_keys.filter((f) =>
    matchesNeedle(
      needle,
      f.name,
      f.columns.join(" "),
      f.references.name,
      f.referenced_columns.join(" "),
    ),
  );
  return (
    <Table head={["Name", "Columns", "References", "On update", "On delete"]}>
      {foreignKeys.map((fk) => (
        <Row key={fk.name} focused={focus === fk.name}>
          <NameCell icon="foreign_key" name={fk.name} />
          <Cell mono>{fk.columns.join(", ")}</Cell>
          <Cell mono>
            {fk.references.name}({fk.referenced_columns.join(", ")})
          </Cell>
          <Cell>{fk.on_update ?? ""}</Cell>
          <Cell>{fk.on_delete ?? ""}</Cell>
        </Row>
      ))}
    </Table>
  );
}

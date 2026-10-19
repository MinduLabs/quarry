import { Plus, Trash2 } from "lucide-react";
import { useId, useState } from "react";
import type { ColumnDef } from "@/bindings/ColumnDef";
import type { ColumnInfo } from "@/bindings/ColumnInfo";
import type { TableRef } from "@/bindings/TableRef";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { IconButton } from "@/components/ui/IconButton";
import { Field, Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { type SchemaForm, useApp } from "@/store/app";

/** Common column types offered while typing; any type the database knows can be typed. */
const TYPES = [
  "INT",
  "INT UNSIGNED",
  "BIGINT",
  "BIGINT UNSIGNED",
  "TINYINT(1)",
  "SMALLINT",
  "DECIMAL(10,2)",
  "DOUBLE",
  "VARCHAR(255)",
  "CHAR(36)",
  "TEXT",
  "MEDIUMTEXT",
  "LONGTEXT",
  "JSON",
  "DATE",
  "TIME",
  "DATETIME",
  "TIMESTAMP",
  "BLOB",
  "BOOLEAN",
];

/**
 * The form that builds a schema change: a column, an index, a new table, a new table name or a
 * new database. Submitting asks the engine for the SQL, which is reviewed before it runs.
 */
export function SchemaFormDialog() {
  const form = useApp((s) => s.schemaForm);
  const close = () => useApp.getState().openSchemaForm(null);
  if (!form) return null;
  // A new key resets the fields when another form opens.
  const key = JSON.stringify(form);
  switch (form.kind) {
    case "column":
      return <ColumnForm key={key} form={form} onClose={close} />;
    case "index":
      return <IndexForm key={key} form={form} onClose={close} />;
    case "create_table":
      return <CreateTableForm key={key} form={form} onClose={close} />;
    case "rename_table":
      return <NameForm key={key} form={form} onClose={close} />;
    case "create_database":
      return <NameForm key={key} form={form} onClose={close} />;
  }
}

function TypeList({ id }: { id: string }) {
  return (
    <datalist id={id}>
      {TYPES.map((t) => (
        <option key={t} value={t} />
      ))}
    </datalist>
  );
}

const blankColumn = (): ColumnDef => ({
  name: "",
  type_name: "VARCHAR(255)",
  nullable: true,
  default: null,
  auto_increment: false,
  on_update: null,
  comment: null,
});

function columnDef(info: ColumnInfo): ColumnDef {
  return {
    name: info.name,
    type_name: info.type_name,
    nullable: info.nullable,
    default: info.default,
    auto_increment: info.auto_increment,
    on_update: info.on_update,
    comment: info.comment,
  };
}

const orNull = (text: string | null) => (text?.trim() ? text.trim() : null);

function ColumnForm({
  form,
  onClose,
}: {
  form: Extract<SchemaForm, { kind: "column" }>;
  onClose: () => void;
}) {
  const [column, setColumn] = useState<ColumnDef>(() =>
    form.column ? columnDef(form.column) : blankColumn(),
  );
  const types = useId();
  const patch = (p: Partial<ColumnDef>) => setColumn((c) => ({ ...c, ...p }));
  const editing = form.column !== null;
  const hasTime = /timestamp|datetime/i.test(column.type_name);
  const ready = column.name.trim() !== "" && column.type_name.trim() !== "";

  const submit = () => {
    if (!ready) return;
    const def = {
      ...column,
      name: column.name.trim(),
      default: orNull(column.default),
      on_update: orNull(column.on_update),
      comment: orNull(column.comment),
    };
    useApp.getState().alterSchema(form.conn, {
      kind: "alter_table",
      table: form.table,
      ops: [
        form.column
          ? { op: "modify_column", name: form.column.name, column: def }
          : { op: "add_column", column: def },
      ],
    });
  };

  return (
    <Dialog
      open
      onClose={onClose}
      title={editing ? `Edit column ${form.column?.name}` : `Add column to ${form.table.name}`}
      width={460}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={submit}>
            Review SQL…
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-3 pb-2"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Field label="Name">
          <Input
            autoFocus
            value={column.name}
            placeholder="created_at"
            onChange={(e) => patch({ name: e.target.value })}
          />
        </Field>
        <Field
          label="Type"
          hint="Any type the database accepts, e.g. VARCHAR(64) or ENUM('a','b')."
        >
          <Input
            list={types}
            value={column.type_name}
            className="font-mono"
            onChange={(e) => patch({ type_name: e.target.value })}
          />
        </Field>
        <TypeList id={types} />
        <Field
          label="Default"
          hint="An SQL expression: 0, 'text', CURRENT_TIMESTAMP. Empty for none."
        >
          <Input
            value={column.default ?? ""}
            placeholder="None"
            className="font-mono"
            onChange={(e) => patch({ default: e.target.value })}
          />
        </Field>
        {(hasTime || column.on_update) && (
          <Field label="On update" hint="Set on every update, e.g. CURRENT_TIMESTAMP.">
            <Input
              value={column.on_update ?? ""}
              placeholder="None"
              className="font-mono"
              onChange={(e) => patch({ on_update: e.target.value })}
            />
          </Field>
        )}
        <Field label="Comment">
          <Input
            value={column.comment ?? ""}
            onChange={(e) => patch({ comment: e.target.value })}
          />
        </Field>
        <Switch
          label="Allow NULL"
          checked={column.nullable}
          onChange={(nullable) => patch({ nullable })}
        />
        <Switch
          label="Auto increment"
          description="Numbers new rows itself. The column must be a key."
          checked={column.auto_increment}
          onChange={(auto_increment) => patch({ auto_increment })}
        />
        <button type="submit" hidden />
      </form>
    </Dialog>
  );
}

function IndexForm({
  form,
  onClose,
}: {
  form: Extract<SchemaForm, { kind: "index" }>;
  onClose: () => void;
}) {
  const info = useApp((s) =>
    s.tabs.find(
      (t) =>
        t.connId === form.conn &&
        t.structure?.info?.table.name === form.table.name &&
        t.structure.info.table.database === form.table.database,
    ),
  )?.structure?.info;
  const all = info?.columns.map((c) => c.name) ?? [];
  const [columns, setColumns] = useState<string[]>(form.columns);
  const [unique, setUnique] = useState(false);
  const suggested = `${unique ? "uq" : "idx"}_${form.table.name}_${columns.join("_") || "…"}`;
  const [name, setName] = useState("");
  const finalName = name.trim() || suggested;
  const ready = columns.length > 0 && !finalName.includes("…");

  const toggle = (column: string, on: boolean) =>
    setColumns((list) => (on ? [...list, column] : list.filter((c) => c !== column)));

  const submit = () => {
    if (!ready) return;
    useApp.getState().alterSchema(form.conn, {
      kind: "alter_table",
      table: form.table,
      ops: [{ op: "add_index", index: { name: finalName, columns, unique } }],
    });
  };

  return (
    <Dialog
      open
      onClose={onClose}
      title={`Add index to ${form.table.name}`}
      description="Columns are indexed in the order you tick them."
      width={460}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={submit}>
            Review SQL…
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 pb-2">
        <Field label="Name">
          <Input
            autoFocus
            value={name}
            placeholder={suggested}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
        </Field>
        <div className="flex flex-col gap-1">
          <span className="text-sm text-muted">Columns</span>
          <div className="max-h-56 overflow-y-auto rounded-md border border-line">
            {all.map((column) => {
              const position = columns.indexOf(column);
              return (
                // biome-ignore lint/a11y/noLabelWithoutControl: the checkbox is inside the label
                <label
                  key={column}
                  className="flex h-7 cursor-pointer items-center gap-2 px-2 text-base hover:bg-hover"
                >
                  <Checkbox
                    checked={position >= 0}
                    onChange={(on) => toggle(column, on)}
                    aria-label={column}
                  />
                  <span className="flex-1 truncate font-mono text-sm">{column}</span>
                  {position >= 0 && (
                    <span className="text-faint text-xs tabular-nums">{position + 1}</span>
                  )}
                </label>
              );
            })}
          </div>
        </div>
        <Switch
          label="Unique"
          description="No two rows can have the same values in these columns."
          checked={unique}
          onChange={setUnique}
        />
      </div>
    </Dialog>
  );
}

interface DraftColumn extends ColumnDef {
  /** Stable key while rows are added and removed. */
  key: number;
  primary: boolean;
}

let nextDraft = 1;

function CreateTableForm({
  form,
  onClose,
}: {
  form: Extract<SchemaForm, { kind: "create_table" }>;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [columns, setColumns] = useState<DraftColumn[]>(() => [
    {
      ...blankColumn(),
      key: nextDraft++,
      name: "id",
      type_name: "BIGINT UNSIGNED",
      nullable: false,
      auto_increment: true,
      primary: true,
    },
  ]);
  const types = useId();
  const patch = (key: number, p: Partial<DraftColumn>) =>
    setColumns((list) => list.map((c) => (c.key === key ? { ...c, ...p } : c)));
  const named = columns.filter((c) => c.name.trim());
  const ready = name.trim() !== "" && named.length > 0 && named.every((c) => c.type_name.trim());

  const submit = () => {
    if (!ready) return;
    const table: TableRef = { database: form.database, schema: null, name: name.trim() };
    useApp.getState().alterSchema(form.conn, {
      kind: "create_table",
      table,
      columns: named.map(({ key: _key, primary, ...c }) => ({
        ...c,
        name: c.name.trim(),
        // A key column can't hold NULL.
        nullable: c.nullable && !primary,
        default: orNull(c.default),
        comment: orNull(c.comment),
      })),
      primary_key: named.filter((c) => c.primary).map((c) => c.name.trim()),
    });
  };

  const head = "px-1 text-left font-normal text-muted text-sm";

  return (
    <Dialog
      open
      onClose={onClose}
      title={`New table in ${form.database}`}
      width={720}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={submit}>
            Review SQL…
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 pb-2">
        <Field label="Name">
          <Input
            autoFocus
            value={name}
            placeholder="orders"
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <TypeList id={types} />
        <table className="w-full border-separate border-spacing-y-1">
          <thead>
            <tr>
              <th className={head}>Column</th>
              <th className={head}>Type</th>
              <th className={head}>Default</th>
              <th className={`${head} w-12 text-center`}>Null</th>
              <th className={`${head} w-12 text-center`}>Key</th>
              <th className={`${head} w-12 text-center`}>Auto</th>
              <th className="w-8">
                <span className="sr-only">Remove</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {columns.map((c) => (
              <tr key={c.key}>
                <td className="px-1">
                  <Input
                    aria-label="Column name"
                    value={c.name}
                    onChange={(e) => patch(c.key, { name: e.target.value })}
                  />
                </td>
                <td className="px-1">
                  <Input
                    aria-label="Type"
                    list={types}
                    value={c.type_name}
                    className="font-mono"
                    onChange={(e) => patch(c.key, { type_name: e.target.value })}
                  />
                </td>
                <td className="px-1">
                  <Input
                    aria-label="Default"
                    value={c.default ?? ""}
                    placeholder="None"
                    className="font-mono"
                    onChange={(e) => patch(c.key, { default: e.target.value })}
                  />
                </td>
                <td className="text-center">
                  <Checkbox
                    aria-label="Allow NULL"
                    checked={c.nullable && !c.primary}
                    disabled={c.primary}
                    onChange={(nullable) => patch(c.key, { nullable })}
                  />
                </td>
                <td className="text-center">
                  <Checkbox
                    aria-label="Primary key"
                    checked={c.primary}
                    onChange={(primary) => patch(c.key, { primary })}
                  />
                </td>
                <td className="text-center">
                  <Checkbox
                    aria-label="Auto increment"
                    checked={c.auto_increment}
                    onChange={(auto_increment) => patch(c.key, { auto_increment })}
                  />
                </td>
                <td>
                  <IconButton
                    label="Remove column"
                    size="sm"
                    disabled={columns.length === 1}
                    onClick={() => setColumns((list) => list.filter((x) => x.key !== c.key))}
                  >
                    <Trash2 size={14} strokeWidth={1.75} />
                  </IconButton>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div>
          <Button
            size="sm"
            variant="ghost"
            onClick={() =>
              setColumns((list) => [
                ...list,
                { ...blankColumn(), key: nextDraft++, primary: false },
              ])
            }
          >
            <Plus size={14} strokeWidth={1.75} />
            Add column
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

/** One name: a table's new name, or a new database. */
function NameForm({
  form,
  onClose,
}: {
  form: Extract<SchemaForm, { kind: "rename_table" | "create_database" }>;
  onClose: () => void;
}) {
  const rename = form.kind === "rename_table";
  const [name, setName] = useState(rename ? form.table.name : "");
  const ready = name.trim() !== "" && (!rename || name.trim() !== form.table.name);
  const submit = () => {
    if (!ready) return;
    useApp
      .getState()
      .alterSchema(
        form.conn,
        rename
          ? { kind: "rename_table", table: form.table, new_name: name.trim() }
          : { kind: "create_database", name: name.trim() },
      );
  };
  return (
    <Dialog
      open
      onClose={onClose}
      title={rename ? `Rename ${form.table.name}` : "New database"}
      width={400}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={submit}>
            Review SQL…
          </Button>
        </>
      }
    >
      <div className="pb-2">
        <Field label="Name">
          <Input
            autoFocus
            value={name}
            onFocus={(e) => e.target.select()}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
        </Field>
      </div>
    </Dialog>
  );
}

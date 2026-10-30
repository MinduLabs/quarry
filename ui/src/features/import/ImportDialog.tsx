import type { OnDuplicate } from "@/bindings/OnDuplicate";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Dialog } from "@/components/ui/Dialog";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Field } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { Spinner } from "@/components/ui/Spinner";
import { useApp } from "@/store/app";
import { fileNameOf, type ImportDialogState, isMappingValid } from "./importModel";

const DELIMITERS = [
  { value: ",", label: "Comma  ," },
  { value: ";", label: "Semicolon  ;" },
  { value: "|", label: "Pipe  |" },
  { value: "\t", label: "Tab" },
];

const DUPLICATES: { value: OnDuplicate; label: string }[] = [
  { value: "fail", label: "Stop and keep nothing" },
  { value: "skip", label: "Skip the row and keep the one in the table" },
  { value: "update", label: "Overwrite the row in the table" },
];

/** Skipping a file column is a choice like any table column, so it needs a value of its own. */
const SKIP = "__skip__";

/** Which file to read, how, and which table column each of its columns goes to. */
export function ImportDialog() {
  const d = useApp((s) => s.importDialog);
  const isRunning = useApp((s) => s.importRun !== null);
  const { closeImport, startImport, dismissImportConfirm } = useApp.getState();
  const canImport =
    d?.path != null &&
    !d.isPreviewing &&
    !d.error &&
    d.tableColumns !== null &&
    isMappingValid(d.mapping);

  return (
    <>
      <Dialog
        open={d !== null && !isRunning}
        onClose={closeImport}
        title="Import"
        description={d ? `Into ${d.table.name}` : undefined}
        width={600}
        height={600}
        flush
        footer={
          <>
            <Button onClick={closeImport}>Cancel</Button>
            <Button variant="primary" disabled={!canImport} onClick={() => startImport()}>
              Import
            </Button>
          </>
        }
      >
        {d && (
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
            <Body d={d} />
          </div>
        )}
      </Dialog>
      <ConfirmDialog
        open={d?.confirm != null && !isRunning}
        title="Confirm the import"
        description={d?.confirm ?? undefined}
        confirmLabel="Import rows"
        onConfirm={() => startImport(true)}
        onCancel={dismissImportConfirm}
      >
        <p className="text-base text-muted">
          Rows go in as one change: if any row is refused, none is kept.
        </p>
      </ConfirmDialog>
    </>
  );
}

function Body({ d }: { d: ImportDialogState }) {
  const { chooseImportFile, setImportOptions } = useApp.getState();
  if (d.path === null) {
    return (
      <div className="flex items-center gap-2 pb-2 text-base text-muted">
        <Spinner size={14} /> Waiting for a file…
      </div>
    );
  }
  return (
    <div className="animate-fade flex flex-col gap-4 pb-2">
      <Field label="File">
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-base text-fg" title={d.path}>
            {fileNameOf(d.path)}
          </span>
          <Button size="sm" disabled={d.isChoosingFile} onClick={() => void chooseImportFile()}>
            Change…
          </Button>
        </div>
      </Field>
      <Field label="Delimiter">
        <Select
          ariaLabel="Delimiter"
          value={d.options.delimiter}
          onChange={(delimiter) => setImportOptions({ delimiter })}
          options={DELIMITERS}
        />
      </Field>
      <Field
        label="When a row's key is already in the table"
        hint={
          d.options.on_duplicate === "update"
            ? "The table's values are replaced and cannot be brought back. You will be asked to confirm."
            : undefined
        }
      >
        <Select
          ariaLabel="When a key already exists"
          value={d.options.on_duplicate}
          onChange={(on_duplicate) => setImportOptions({ on_duplicate })}
          options={DUPLICATES}
        />
      </Field>
      <div className="flex flex-col gap-2">
        <Option
          label="The first line holds the column names"
          checked={d.options.has_header}
          onChange={(has_header) => setImportOptions({ has_header })}
        />
        <Option
          label="Store an empty value as NULL"
          checked={d.options.is_empty_null}
          onChange={(is_empty_null) => setImportOptions({ is_empty_null })}
        />
      </div>
      {d.error ? <ErrorNotice message={d.error} /> : <Mapping d={d} />}
    </div>
  );
}

function Option({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: the checkbox is inside the label
    <label className="flex cursor-pointer items-center gap-2 text-base text-fg">
      <Checkbox checked={checked} onChange={onChange} />
      {label}
    </label>
  );
}

function Mapping({ d }: { d: ImportDialogState }) {
  const { setImportColumn } = useApp.getState();
  if (d.isPreviewing || d.tableColumns === null) {
    return (
      <div className="flex items-center gap-2 text-base text-muted">
        <Spinner size={14} /> Reading the file…
      </div>
    );
  }
  const options = [
    { value: SKIP, label: "Skip this column" },
    ...d.tableColumns.map((name) => ({ value: name, label: name })),
  ];
  const used = d.mapping.filter((c) => c !== null);
  const isRepeated = new Set(used.map((c) => c?.toLowerCase())).size !== used.length;
  return (
    <div className="flex flex-col gap-2">
      <span className="text-sm text-muted">
        Columns · the first {d.rows.length.toLocaleString()} rows were read for this preview
      </span>
      {Array.from({ length: d.width }, (_, index) => {
        const name = d.header[index]?.trim() || `Column ${index + 1}`;
        const sample = d.rows[0]?.[index];
        return (
          // biome-ignore lint/suspicious/noArrayIndexKey: a file column is its position, and the list never reorders
          <div key={index} className="flex items-center gap-3">
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-base text-fg">{name}</span>
              {sample !== undefined && (
                <span className="truncate text-muted text-sm" title={sample}>
                  {sample === "" ? "(empty)" : sample}
                </span>
              )}
            </div>
            <div className="w-56 shrink-0">
              <Select
                ariaLabel={`Table column for ${name}`}
                value={d.mapping[index] ?? SKIP}
                onChange={(value) => setImportColumn(index, value === SKIP ? null : value)}
                options={options}
              />
            </div>
          </div>
        );
      })}
      {isRepeated && (
        <span className="text-sm text-warning">
          Each table column can take only one file column.
        </span>
      )}
      {used.length === 0 && (
        <span className="text-sm text-muted">Choose at least one table column.</span>
      )}
    </div>
  );
}

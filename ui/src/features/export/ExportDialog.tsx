import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Field, Input } from "@/components/ui/Input";
import { Select } from "@/components/ui/Select";
import { Spinner } from "@/components/ui/Spinner";
import { useApp } from "@/store/app";
import {
  EXCEL_SLOW_ROWS,
  type ExportDialogState,
  expectedRows,
  exportOptions,
  FORMAT_LABELS,
  formatState,
  needsTargetTable,
  parseLimit,
  unavailableReasons,
} from "./exportModel";

const DELIMITERS = [
  { value: ",", label: "Comma  ," },
  { value: ";", label: "Semicolon  ;" },
  { value: "|", label: "Pipe  |" },
];

/** Where an export's rows go, in what format, how many, and a preview of the file. */
export function ExportDialog() {
  const d = useApp((s) => s.exportDialog);
  const { closeExport, startExport } = useApp.getState();
  const info = d && formatState(d);
  const limit = d ? parseLimit(d.limit) : null;
  const canExport =
    d?.isPrepared === true &&
    !d.error &&
    info?.is_available === true &&
    limit !== "invalid" &&
    !(needsTargetTable(d) && exportOptions(d).table === null) &&
    !d.isChoosingFile;

  return (
    <Dialog
      open={d !== null}
      onClose={closeExport}
      title="Export"
      description={d ? describe(d) : undefined}
      width={600}
      height={620}
      flush
      footer={
        <>
          <Button onClick={closeExport}>Cancel</Button>
          <Button variant="primary" disabled={!canExport} onClick={() => void startExport()}>
            {d?.isChoosingFile && <Spinner size={11} />}
            Export
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
  );
}

function describe(d: ExportDialogState): string {
  if (d.from.kind === "rows") {
    const n = d.from.rows.length;
    return `${d.from.name} · ${n === 1 ? "1 row" : `${n.toLocaleString()} rows`} on the grid`;
  }
  if (d.count !== null) return `${d.from.name} · ${d.count.toLocaleString()} rows`;
  return `${d.from.name} · ${d.isCounting ? "counting rows…" : "rows"}`;
}

function Body({ d }: { d: ExportDialogState }) {
  if (d.error) {
    return (
      <div className="pb-2">
        <ErrorNotice message={d.error} />
      </div>
    );
  }
  if (!d.isPrepared) {
    return (
      <div className="flex items-center gap-2 pb-2 text-base text-muted">
        <Spinner size={14} /> Reading what can be exported…
      </div>
    );
  }
  return (
    <div className="animate-fade flex flex-col gap-4 pb-2">
      <FormatField d={d} />
      <RowsField d={d} />
      <OptionFields d={d} />
      <Notes d={d} />
      <Preview d={d} />
    </div>
  );
}

function FormatField({ d }: { d: ExportDialogState }) {
  const { setExportChoice } = useApp.getState();
  const reasons = unavailableReasons(d.formats);
  return (
    <Field label="Format">
      <Select
        ariaLabel="Format"
        value={d.format}
        onChange={(format) => setExportChoice({ format })}
        options={d.formats.map((f) => ({
          value: f.format,
          label: FORMAT_LABELS[f.format],
          disabled: !f.is_available,
        }))}
      />
      {reasons.map(({ labels, reason }) => (
        <span key={reason} className="text-sm text-muted">
          {labels.join(" and ")} {labels.length === 1 ? "is" : "are"} not available. {reason}
        </span>
      ))}
    </Field>
  );
}

function RowsField({ d }: { d: ExportDialogState }) {
  const { setExportChoice } = useApp.getState();
  const limit = parseLimit(d.limit);
  const available =
    d.from.kind === "table" ? d.count : d.scope === "grid" ? d.from.rows.length : null;
  const hint = (() => {
    if (limit === "invalid") return "Type a whole number above zero, or leave it empty for all.";
    if (d.from.kind === "table" && d.isCounting && d.count === null) {
      return "Counting rows… you can export without waiting.";
    }
    if (available !== null) return `Leave empty for all ${available.toLocaleString()} rows.`;
    return "Leave empty for every row.";
  })();
  return (
    <div className="flex flex-col gap-3">
      {d.from.kind === "rows" && d.from.query && (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1 text-muted text-sm">Rows</legend>
          <RadioRow
            checked={d.scope === "grid"}
            onSelect={() => setExportChoice({ scope: "grid" })}
            label={`The ${d.from.rows.length.toLocaleString()} rows on the grid`}
            hint="The grid keeps only the first rows of a long result."
          />
          <RadioRow
            checked={d.scope === "query"}
            disabled={d.queryError !== null}
            onSelect={() => setExportChoice({ scope: "query" })}
            label="Every row of the query"
            hint={d.queryError ?? "Runs the query again and writes all its rows."}
          />
        </fieldset>
      )}
      <Field label="Rows to export" hint={hint}>
        <Input
          inputMode="numeric"
          placeholder="All"
          value={d.limit}
          aria-invalid={limit === "invalid"}
          onChange={(e) => setExportChoice({ limit: e.target.value })}
        />
      </Field>
    </div>
  );
}

function RadioRow({
  checked,
  disabled,
  onSelect,
  label,
  hint,
}: {
  checked: boolean;
  disabled?: boolean;
  onSelect: () => void;
  label: string;
  hint: string;
}) {
  return (
    <label
      className="flex items-start gap-2 data-[disabled=true]:opacity-45"
      data-disabled={disabled}
    >
      <input
        type="radio"
        checked={checked}
        disabled={disabled}
        onChange={onSelect}
        className="mt-1 size-3.5 shrink-0 cursor-pointer accent-accent disabled:cursor-default"
      />
      <span className="flex flex-col">
        <span className="text-base text-fg">{label}</span>
        <span className="text-sm text-muted">{hint}</span>
      </span>
    </label>
  );
}

function OptionFields({ d }: { d: ExportDialogState }) {
  const { setExportChoice } = useApp.getState();
  const set = (patch: Partial<ExportDialogState["options"]>) =>
    setExportChoice({ options: { ...d.options, ...patch } });
  const header = (
    <CheckRow
      label="First row holds the column names"
      checked={d.options.has_header}
      onChange={(has_header) => set({ has_header })}
    />
  );
  const bom = (
    <CheckRow
      label="Add a byte order mark, so Excel reads accents correctly"
      checked={d.options.has_bom}
      onChange={(has_bom) => set({ has_bom })}
    />
  );
  switch (d.format) {
    case "csv":
      return (
        <div className="flex flex-col gap-2">
          <Field label="Separator">
            <Select
              ariaLabel="Separator"
              value={d.options.delimiter}
              onChange={(delimiter) => set({ delimiter })}
              options={DELIMITERS}
            />
          </Field>
          {header}
          {bom}
        </div>
      );
    case "tsv":
      return (
        <div className="flex flex-col gap-2">
          {header}
          {bom}
        </div>
      );
    case "xlsx":
      return (
        <div className="flex flex-col gap-2">
          {header}
          <Field
            label="Rows per sheet"
            hint="A longer result continues on a new sheet. Leave empty to fill each sheet."
          >
            <Input
              inputMode="numeric"
              placeholder="1,048,575"
              value={
                d.options.max_rows_per_sheet === null ? "" : String(d.options.max_rows_per_sheet)
              }
              onChange={(e) => {
                const n = parseLimit(e.target.value);
                set({ max_rows_per_sheet: typeof n === "number" ? n : null });
              }}
            />
          </Field>
        </div>
      );
    case "sql_inserts":
      return d.table ? (
        <p className="text-base text-muted">
          Statements insert into{" "}
          <span className="font-mono text-fg">
            {[d.table.database, d.table.name].filter(Boolean).join(".")}
          </span>
        </p>
      ) : (
        <Field
          label="Insert into table"
          hint="The rows come from several tables. Name the table they go into, as in orders or shop.orders."
        >
          <Input
            value={d.targetTable}
            aria-invalid={d.targetTable !== "" && exportOptions(d).table === null}
            onChange={(e) => setExportChoice({ targetTable: e.target.value })}
          />
        </Field>
      );
    default:
      return null;
  }
}

function CheckRow({
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

function Notes({ d }: { d: ExportDialogState }) {
  const rows = expectedRows(d);
  const notes: string[] = [];
  if (d.scope === "query" && d.from.kind === "rows" && d.from.query) {
    notes.push(
      "The query runs again on its own connection, so the file can differ from the grid: the data may have changed since, and variables or temporary tables of this tab are not there.",
      ...d.warnings,
    );
  }
  if (d.format === "xlsx" && rows !== null && rows > EXCEL_SLOW_ROWS) {
    notes.push(
      `Excel opens ${rows.toLocaleString()} rows slowly. CSV opens faster and holds any number of rows.`,
    );
  }
  if (notes.length === 0) return null;
  return (
    <div className="flex flex-col gap-1.5 rounded-md bg-surface p-3 text-muted text-sm">
      {notes.map((note) => (
        <p key={note}>{note}</p>
      ))}
    </div>
  );
}

function Preview({ d }: { d: ExportDialogState }) {
  const info = formatState(d);
  if (!info?.has_preview) {
    return (
      <p className="text-muted text-sm">Excel files have no preview. The file opens in Excel.</p>
    );
  }
  const { text, error, loading } = d.preview;
  return (
    <div className="flex flex-col gap-1">
      <span className="flex h-4 items-center gap-2 text-muted text-sm">
        Preview of the first rows
        <Spinner size={11} className={loading ? "" : "invisible"} />
      </span>
      {error ? (
        <ErrorNotice message={error} />
      ) : (
        <pre className="h-36 select-text overflow-auto whitespace-pre rounded-md bg-surface p-3 font-mono text-fg text-sm">
          {text ?? ""}
        </pre>
      )}
    </div>
  );
}

import { ArrowRight, Copy } from "lucide-react";
import type { SchemaSide } from "@/bindings/SchemaSide";
import type { TableDiff } from "@/bindings/TableDiff";
import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Dialog } from "@/components/ui/Dialog";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { IconButton } from "@/components/ui/IconButton";
import { Select } from "@/components/ui/Select";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { copyText, useApp } from "@/store/app";

const KIND_LABEL: Record<TableDiff["kind"], string> = {
  missing: "Only in source · created",
  extra: "Only in target · dropped",
  changed: "Changed",
};

/**
 * Compare the tables of two databases and make the target match the source. The engine reads
 * both and writes the script; nothing runs until its review is confirmed.
 */
export function SchemaCompareDialog() {
  const compare = useApp((s) => s.schemaCompare);
  const { closeSchemaCompare, runSchemaCompare, reviewSchemaDiff } = useApp.getState();
  const diff = compare?.diff ?? null;
  const isRunning = compare?.request !== null && compare?.request !== undefined;
  const canApply = (diff?.statements.length ?? 0) > 0;

  return (
    <>
      <Dialog
        open={Boolean(compare)}
        onClose={closeSchemaCompare}
        title="Compare schemas"
        description="What the target needs to match the source: tables, columns and indexes."
        width={820}
        footer={
          <>
            <Button onClick={closeSchemaCompare}>Close</Button>
            <Button variant="primary" disabled={!canApply} onClick={reviewSchemaDiff}>
              Apply to target…
            </Button>
          </>
        }
      >
        {compare && (
          <div className="flex min-h-0 flex-col gap-4 pb-2">
            <div className="flex items-end gap-3">
              <SidePicker label="Source" which="source" side={compare.source} />
              <ArrowRight size={16} strokeWidth={1.75} className="mb-1.5 shrink-0 text-muted" />
              <SidePicker label="Target" which="target" side={compare.target} />
              <Button
                variant="primary"
                disabled={!compare.source || !compare.target || isRunning}
                onClick={runSchemaCompare}
              >
                {isRunning && <Spinner size={11} />}
                Compare
              </Button>
            </div>
            {compare.error && <ErrorNotice message={compare.error} />}
            {diff && <Result />}
          </div>
        )}
      </Dialog>
      <ApplyReview />
    </>
  );
}

/** A connection and one of its databases. Only connected connections list their databases. */
function SidePicker({
  label,
  which,
  side,
}: {
  label: string;
  which: "source" | "target";
  side: SchemaSide | null;
}) {
  const connections = useApp((s) => s.connections);
  const nodes = useApp((s) => s.nodes);
  const { setCompareSide } = useApp.getState();
  const connected = connections.filter((c) => nodes[c.id]?.status.kind === "connected");
  const databases = side ? (nodes[side.conn]?.databases ?? []).map((d) => d.name) : [];

  if (connected.length === 0) {
    return <p className="flex-1 text-muted text-sm">Connect to a database first.</p>;
  }
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1.5">
      <span className="text-muted text-sm">{label}</span>
      <div className="grid grid-cols-2 gap-2">
        <Select
          ariaLabel={`${label} connection`}
          value={side?.conn ?? ""}
          options={connected.map((c) => ({ value: c.id, label: c.name }))}
          onChange={(conn) => {
            const first = nodes[conn]?.databases[0]?.name;
            if (first !== undefined) setCompareSide(which, { conn, database: first });
          }}
        />
        <Select
          ariaLabel={`${label} database`}
          value={side?.database ?? ""}
          options={databases.map((d) => ({ value: d, label: d }))}
          onChange={(database) => side && setCompareSide(which, { conn: side.conn, database })}
        />
      </div>
    </div>
  );
}

function Result() {
  const diff = useApp((s) => s.schemaCompare?.diff);
  if (!diff) return null;
  if (diff.tables.length === 0) {
    return (
      <p className="text-base text-success">
        The databases match ({diff.same} {diff.same === 1 ? "table" : "tables"}).
      </p>
    );
  }
  const script = diff.statements.map((s) => `${s};`).join("\n\n");
  return (
    <div className="flex min-h-0 flex-col gap-3">
      <p className="text-base text-fg" role="status">
        {diff.tables.length} {diff.tables.length === 1 ? "table differs" : "tables differ"} ·{" "}
        {diff.same} the same
      </p>
      <ul className="flex max-h-56 flex-col gap-2 overflow-auto">
        {diff.tables.map((table) => (
          <li key={table.name} className="rounded-md border border-line p-2">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-fg">{table.name}</span>
              <span
                className={cn("text-xs", table.kind === "extra" ? "text-danger" : "text-muted")}
              >
                {KIND_LABEL[table.kind]}
              </span>
            </div>
            <ul className="mt-1 font-mono text-muted text-sm">
              {table.details.map((line) => (
                <li key={line} className="whitespace-pre-wrap">
                  {line}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
      {script ? (
        <div className="flex min-h-0 flex-col gap-1.5">
          <div className="flex items-center justify-between">
            <span className="text-muted text-sm">Script for the target</span>
            <IconButton label="Copy script" onClick={() => copyText(script, "Copied the script")}>
              <Copy size={14} strokeWidth={1.75} />
            </IconButton>
          </div>
          <pre className="max-h-48 select-text overflow-auto whitespace-pre-wrap rounded-md bg-surface p-3 font-mono text-fg text-sm">
            {script}
          </pre>
        </div>
      ) : (
        <p className="text-muted text-sm">Nothing here can be scripted yet.</p>
      )}
    </div>
  );
}

/** The engine's review of the script: it runs on the target only once this is confirmed. */
function ApplyReview() {
  const review = useApp((s) => s.schemaCompare?.review ?? null);
  const target = useApp((s) => s.schemaCompare?.diff?.target);
  const name = useApp((s) => s.connections.find((c) => c.id === target?.conn)?.name);
  const { answerSchemaDiff } = useApp.getState();
  const count = review?.statements.length ?? 0;

  return (
    <ConfirmDialog
      open={Boolean(review)}
      title={`Change ${target?.database ?? "the target"}?`}
      description={review?.warning ?? "Review the SQL before it runs."}
      confirmLabel={
        review?.running ? "Running…" : `Run ${count} ${count === 1 ? "statement" : "statements"}`
      }
      tone={review?.warning ? "danger" : "primary"}
      width={640}
      onConfirm={() => answerSchemaDiff(true)}
      onCancel={() => answerSchemaDiff(false)}
    >
      <div className="flex flex-col gap-2 pb-2">
        <p className="text-muted text-sm">
          Connection: {name} · {target?.database}
        </p>
        <pre className="max-h-72 select-text overflow-auto whitespace-pre-wrap rounded-md bg-surface p-3 font-mono text-fg text-sm">
          {review?.statements.map((s) => `${s};`).join("\n\n")}
        </pre>
        {review?.error && <ErrorNotice message={review.error} />}
      </div>
    </ConfirmDialog>
  );
}

import type { SchemaChange } from "@/bindings/SchemaChange";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { ErrorNotice } from "@/components/ui/ErrorNotice";
import { Spinner } from "@/components/ui/Spinner";
import { useApp } from "@/store/app";

/** The SQL of a schema change, shown before it runs. Nothing runs until Run. */
export function SchemaReviewDialog() {
  const review = useApp((s) => s.schemaReview);
  const name = useApp((s) => s.connections.find((c) => c.id === review?.conn)?.name);
  const answer = useApp((s) => s.answerSchemaReview);
  const risky = Boolean(review?.warning);

  return (
    <Dialog
      open={Boolean(review)}
      onClose={() => answer(false)}
      title={review ? titleOf(review.change) : ""}
      description={review?.warning ?? "Review the SQL before it runs."}
      width={560}
      footer={
        <>
          <Button autoFocus={risky} onClick={() => answer(false)}>
            Cancel
          </Button>
          <Button
            variant={risky ? "danger" : "primary"}
            autoFocus={!risky}
            disabled={review?.running}
            onClick={() => answer(true)}
          >
            {review?.running && <Spinner size={11} />}
            Run
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2 pb-2">
        <p className="text-sm text-muted">Connection: {name}</p>
        <pre className="max-h-72 select-text overflow-auto whitespace-pre-wrap rounded-md bg-surface p-3 font-mono text-sm text-fg">
          {review?.statements.map((s) => `${s};`).join("\n\n")}
        </pre>
        {review?.error && <ErrorNotice message={review.error} />}
      </div>
    </Dialog>
  );
}

function titleOf(change: SchemaChange): string {
  switch (change.kind) {
    case "create_database":
      return "Create database";
    case "drop_database":
      return `Drop database ${change.name}?`;
    case "create_table":
      return "Create table";
    case "rename_table":
      return "Rename table";
    case "truncate_table":
      return `Empty ${change.table.name}?`;
    case "drop_table":
      return `Drop ${change.view ? "view" : "table"} ${change.table.name}?`;
    case "alter_table":
      return change.ops.some((op) => op.op === "drop_column" || op.op === "drop_index")
        ? `Change ${change.table.name}?`
        : `Change ${change.table.name}`;
  }
}

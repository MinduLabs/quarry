import { useId, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import type { QueryVariable } from "@/lib/variables";

/** Ask for the values of the `{{variables}}`, `:names` and bare `?` marks in a query, then run it. */
export function VariablesDialog({
  variables,
  initial,
  onRun,
  onClose,
}: {
  variables: QueryVariable[];
  /** Values from the last run of this tab, so a repeated run is one Enter away. */
  initial: Record<string, string>;
  onRun: (values: Record<string, string>) => void;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, string>>(initial);
  const submit = () =>
    onRun(Object.fromEntries(variables.map((v) => [v.name, values[v.name] ?? ""])));

  return (
    <Dialog
      open
      onClose={onClose}
      title="Query variables"
      description="Values go into the query as typed. Add quotes in the query where a text value needs them."
      width={420}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit}>
            Run
          </Button>
        </>
      }
    >
      {/* The padding keeps the focus ring of the first and last input inside the scrolling body. */}
      <div className="flex flex-col gap-3 py-1">
        {variables.map((variable, i) => (
          <VariableField
            key={variable.name}
            variable={variable}
            isFirst={i === 0}
            value={values[variable.name] ?? ""}
            onChange={(value) => setValues((v) => ({ ...v, [variable.name]: value }))}
            onSubmit={submit}
          />
        ))}
      </div>
    </Dialog>
  );
}

function VariableField({
  variable,
  isFirst,
  value,
  onChange,
  onSubmit,
}: {
  variable: QueryVariable;
  isFirst: boolean;
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  const id = useId();
  return (
    <div className="flex items-center gap-3">
      <label
        htmlFor={id}
        title={variable.hint}
        className="flex min-w-0 basis-1/2 items-baseline gap-2 font-mono text-sm"
      >
        <span className="shrink-0">{variable.label}</span>
        {variable.hint && <span className="truncate text-muted">{variable.hint}</span>}
      </label>
      <Input
        id={id}
        className="basis-1/2"
        autoFocus={isFirst}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onSubmit();
        }}
      />
    </div>
  );
}

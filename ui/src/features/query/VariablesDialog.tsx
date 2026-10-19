import { useId, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";

/** Ask for the values of the `{{variables}}` in a query, then run it. */
export function VariablesDialog({
  names,
  initial,
  onRun,
  onClose,
}: {
  names: string[];
  /** Values from the last run of this tab, so a repeated run is one Enter away. */
  initial: Record<string, string>;
  onRun: (values: Record<string, string>) => void;
  onClose: () => void;
}) {
  const [values, setValues] = useState<Record<string, string>>(initial);
  const submit = () => onRun(Object.fromEntries(names.map((n) => [n, values[n] ?? ""])));

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
      <div className="flex flex-col gap-3">
        {names.map((name, i) => (
          <VariableField
            key={name}
            name={name}
            isFirst={i === 0}
            value={values[name] ?? ""}
            onChange={(value) => setValues((v) => ({ ...v, [name]: value }))}
            onSubmit={submit}
          />
        ))}
      </div>
    </Dialog>
  );
}

function VariableField({
  name,
  isFirst,
  value,
  onChange,
  onSubmit,
}: {
  name: string;
  isFirst: boolean;
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="font-mono text-muted text-sm">
        {name}
      </label>
      <Input
        id={id}
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

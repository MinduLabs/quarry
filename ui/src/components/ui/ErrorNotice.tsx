import { Check, CircleAlert, Copy } from "lucide-react";
import { useState } from "react";
import { DetailsToggle } from "./DetailsToggle";
import { IconButton } from "./IconButton";

/**
 * A failure on its own row. `message` is the engine's multi-line error:
 * the first line says what happened, the next lines why, and a `Code:` line holds the exact code.
 * Only the first line shows until the reader asks for the details.
 */
export function ErrorNotice({ message }: { message: string }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [headline, ...rest] = message.split("\n");
  const code = rest.find((line) => line.startsWith("Code: "));
  const why = rest.filter((line) => line !== code);

  const copy = () => {
    navigator.clipboard
      .writeText(message)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  return (
    <div
      role="alert"
      className="flex items-start gap-2 rounded-md border border-danger/40 bg-danger/10 py-2 pr-1.5 pl-3 text-sm"
    >
      <CircleAlert size={14} strokeWidth={1.75} className="mt-0.5 shrink-0 text-danger" />
      <div className="min-w-0 flex-1 select-text">
        <p className="break-words font-medium text-fg">{headline}</p>
        {rest.length > 0 && <DetailsToggle open={open} onToggle={() => setOpen(!open)} />}
        {open && (
          <>
            {why.map((line) => (
              <p key={line} className="mt-1 break-words text-muted">
                {line}
              </p>
            ))}
            {code && <p className="mt-1.5 break-all font-mono text-muted text-xs">{code}</p>}
          </>
        )}
      </div>
      <IconButton label={copied ? "Copied" : "Copy error"} size="sm" onClick={copy}>
        {copied ? <Check size={14} strokeWidth={1.75} /> : <Copy size={14} strokeWidth={1.75} />}
      </IconButton>
    </div>
  );
}

import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import { useState } from "react";
import { cn } from "@/lib/cn";
import { useApp } from "@/store/app";
import { DetailsToggle } from "./DetailsToggle";

const icons = { info: Info, success: CircleCheck, warning: TriangleAlert, error: CircleAlert };
const colors = {
  info: "text-accent-text",
  success: "text-success",
  warning: "text-warning",
  error: "text-danger",
};

export function Toaster() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed right-4 bottom-8 z-50 flex w-90 flex-col gap-2">
      {toasts.map((toast) => {
        const Icon = icons[toast.kind];
        return (
          <div
            key={toast.id}
            role={toast.kind === "error" ? "alert" : "status"}
            className="animate-toast pointer-events-auto flex items-start gap-2.5 rounded-lg bg-elevated px-3 py-2.5 shadow-popover"
          >
            <Icon
              size={16}
              strokeWidth={1.75}
              className={cn("mt-px shrink-0", colors[toast.kind])}
            />
            <ToastText message={toast.message} />
            <button
              type="button"
              aria-label="Dismiss"
              onClick={() => dismiss(toast.id)}
              className="-mr-1 inline-flex size-5 items-center justify-center rounded-sm text-muted hover:bg-hover hover:text-fg"
            >
              <X size={12} strokeWidth={2} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** First line always; the rest (cause, code) behind a "Details" toggle. */
function ToastText({ message }: { message: string }) {
  const [open, setOpen] = useState(false);
  const [headline, ...rest] = message.split("\n");
  return (
    <div className="min-w-0 flex-1 select-text text-base text-fg">
      <p className="break-words">{headline}</p>
      {rest.length > 0 && <DetailsToggle open={open} onToggle={() => setOpen(!open)} />}
      {open && (
        <p className="mt-1 whitespace-pre-line break-words text-muted text-sm">{rest.join("\n")}</p>
      )}
    </div>
  );
}

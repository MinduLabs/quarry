import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/cn";

/** "Details" under a message's first line; the caller shows the rest while it is open. */
export function DetailsToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      aria-expanded={open}
      onClick={onToggle}
      className="mt-0.5 -ml-0.5 inline-flex items-center gap-0.5 rounded-sm text-muted text-xs transition-colors-fast hover:text-fg"
    >
      <ChevronRight
        size={12}
        strokeWidth={2}
        className={cn(
          "transition-transform duration-(--duration-fast) ease-(--ease-out)",
          open && "rotate-90",
        )}
      />
      {open ? "Hide details" : "Details"}
    </button>
  );
}

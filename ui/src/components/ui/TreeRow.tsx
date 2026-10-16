import { ChevronRight } from "lucide-react";
import { forwardRef, type HTMLAttributes, type ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface TreeRowProps extends HTMLAttributes<HTMLDivElement> {
  depth: number;
  /** `undefined` = not expandable (no chevron). */
  expanded?: boolean;
  icon: ReactNode;
  label: string;
  active?: boolean;
  trailing?: ReactNode;
}

/** One 28px row of the sidebar tree. */
export const TreeRow = forwardRef<HTMLDivElement, TreeRowProps>(function TreeRow(
  { depth, expanded, icon, label, active, trailing, className, ...props },
  ref,
) {
  return (
    <div
      ref={ref}
      role="treeitem"
      aria-expanded={expanded}
      aria-selected={active}
      tabIndex={-1}
      style={{ paddingLeft: 6 + depth * 12 }}
      className={cn(
        "group flex h-7 items-center gap-1.5 rounded-md pr-2 text-base transition-colors-fast",
        active ? "bg-selected text-fg" : "text-fg hover:bg-hover",
        className,
      )}
      {...props}
    >
      <span className="flex size-3 shrink-0 items-center justify-center text-faint">
        {expanded !== undefined && (
          <ChevronRight
            size={12}
            strokeWidth={2}
            className={cn(
              "transition-transform duration-(--duration-fast) ease-(--ease-out)",
              expanded && "rotate-90",
            )}
          />
        )}
      </span>
      <span className="flex shrink-0 text-muted">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
    </div>
  );
});

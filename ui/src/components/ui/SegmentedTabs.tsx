import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface SegmentedTab<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
}

/** A small two-or-three way switch for panes that share one area (e.g. the sidebar). */
export function SegmentedTabs<T extends string>({
  value,
  onChange,
  tabs,
  label,
  className,
}: {
  value: T;
  onChange: (value: T) => void;
  tabs: SegmentedTab<T>[];
  label: string;
  className?: string;
}) {
  return (
    <div
      role="tablist"
      aria-label={label}
      className={cn("flex h-8 items-center gap-0.5 rounded-lg bg-hover p-0.5", className)}
    >
      {tabs.map((tab) => {
        const active = tab.value === value;
        return (
          <button
            key={tab.value}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(tab.value)}
            className={cn(
              "flex h-7 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md px-2 text-sm transition-colors-fast",
              active ? "bg-app text-fg shadow-[0_0_0_1px_var(--line)]" : "text-muted hover:text-fg",
            )}
          >
            {tab.icon}
            <span className="truncate">{tab.label}</span>
          </button>
        );
      })}
    </div>
  );
}

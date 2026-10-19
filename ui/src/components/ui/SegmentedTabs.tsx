import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface SegmentedTab<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
}

/**
 * A small two-or-three way switch for panes that share one area (e.g. the sidebar). `sm` fits a
 * 32px bar (24px tall).
 */
export function SegmentedTabs<T extends string>({
  value,
  onChange,
  tabs,
  label,
  size = "md",
  className,
}: {
  value: T;
  onChange: (value: T) => void;
  tabs: SegmentedTab<T>[];
  label: string;
  size?: "sm" | "md";
  className?: string;
}) {
  const small = size === "sm";
  return (
    <div
      role="tablist"
      aria-label={label}
      className={cn(
        "flex items-center gap-0.5 bg-hover p-0.5",
        small ? "h-6 rounded-md" : "h-8 rounded-lg",
        className,
      )}
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
              "flex min-w-0 flex-1 items-center justify-center gap-1.5 px-2 transition-colors-fast",
              small ? "h-5 rounded-sm text-xs" : "h-7 rounded-md text-sm",
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

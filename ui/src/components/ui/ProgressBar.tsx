import { cn } from "@/lib/cn";

/**
 * A thin bar for work with a known end. `value` from 0 to 1; `null` while the amount is not
 * known yet, which slides a segment instead. Scaled and moved, never resized: only transform
 * animates smoothly in WebKitGTK.
 */
export function ProgressBar({ value, className }: { value: number | null; className?: string }) {
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value === null ? undefined : Math.round(value * 100)}
      className={cn("h-1 w-full overflow-hidden rounded-full bg-hover", className)}
    >
      {value === null ? (
        <div className="animate-indeterminate h-full w-1/3 rounded-full bg-accent" />
      ) : (
        <div
          className="h-full origin-left rounded-full bg-accent transition-transform duration-150"
          style={{ transform: `scaleX(${Math.min(Math.max(value, 0), 1)})` }}
        />
      )}
    </div>
  );
}

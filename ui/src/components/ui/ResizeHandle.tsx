import type { PointerEvent as ReactPointerEvent } from "react";
import { cn } from "@/lib/cn";

/**
 * The hairline between two panes; dragging it resizes them. `onStart` runs when a drag starts
 * (read the size to start from there) and returns what each move does with how far the pointer
 * has gone since, in pixels. `horizontal` lies across and moves up and down.
 */
export function ResizeHandle({
  orientation,
  onStart,
}: {
  orientation: "horizontal" | "vertical";
  onStart: () => (delta: number) => void;
}) {
  const isRow = orientation === "horizontal";
  const start = (event: ReactPointerEvent) => {
    event.preventDefault();
    const origin = isRow ? event.clientY : event.clientX;
    const resize = onStart();
    const move = (e: PointerEvent) => resize((isRow ? e.clientY : e.clientX) - origin);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  return (
    <div
      role="separator"
      aria-orientation={orientation}
      onPointerDown={start}
      className={cn(
        "group relative shrink-0 bg-line",
        isRow ? "h-px cursor-row-resize" : "w-px cursor-col-resize",
      )}
    >
      {/* A wider grip than the line, so it is easy to catch. */}
      <div
        className={cn(
          "absolute transition-colors-fast group-hover:bg-accent-soft",
          isRow ? "inset-x-0 -top-1 h-2" : "inset-y-0 -left-1 w-2",
        )}
      />
    </div>
  );
}

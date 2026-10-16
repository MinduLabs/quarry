import { type ButtonHTMLAttributes, forwardRef, type ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Tooltip } from "./Tooltip";

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Accessible name; also shown as the tooltip. */
  label: string;
  shortcut?: string;
  size?: "sm" | "md";
  children: ReactNode;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, shortcut, size = "md", className, type = "button", children, ...props },
  ref,
) {
  return (
    <Tooltip content={label} shortcut={shortcut}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-md text-muted",
          "transition-colors-fast hover:bg-hover hover:text-fg active:bg-pressed",
          "disabled:pointer-events-none disabled:opacity-40",
          size === "sm" ? "size-6" : "size-7",
          className,
        )}
        {...props}
      >
        {children}
      </button>
    </Tooltip>
  );
});

import type { InputHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

/** A native checkbox in the accent color. Give it an `aria-label` when it has no visible label. */
export function Checkbox({
  className,
  onChange,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> & {
  onChange: (checked: boolean) => void;
}) {
  return (
    <input
      type="checkbox"
      onChange={(e) => onChange(e.target.checked)}
      className={cn(
        "size-3.5 shrink-0 cursor-pointer accent-accent disabled:cursor-default disabled:opacity-45",
        "focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-1",
        className,
      )}
      {...props}
    />
  );
}

import { Check, Minus } from "lucide-react";
import { type InputHTMLAttributes, useEffect, useRef } from "react";
import { cn } from "@/lib/cn";

/**
 * A checkbox drawn to match the app: a hairline box that fills with the accent when checked.
 * The native input stays underneath for the keyboard and screen readers; WebKitGTK draws its own
 * box heavy and square. `indeterminate` shows a dash, for a box that stands for a list some of
 * whose items are ticked. Give it an `aria-label` when it has no visible label.
 */
export function Checkbox({
  className,
  onChange,
  indeterminate = false,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> & {
  onChange: (checked: boolean) => void;
  indeterminate?: boolean;
}) {
  const ref = useRef<HTMLInputElement>(null);
  // Only a property, not an attribute: it cannot be set in markup.
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <span className={cn("relative inline-flex size-3.5 shrink-0", className)}>
      <input
        ref={ref}
        type="checkbox"
        onChange={(e) => onChange(e.target.checked)}
        className={cn(
          "peer absolute inset-0 m-0 cursor-pointer appearance-none rounded-[4px] border",
          "border-line-strong bg-app transition-colors-fast hover:border-faint",
          "checked:border-accent checked:bg-accent checked:hover:border-accent-hover checked:hover:bg-accent-hover",
          "indeterminate:border-accent indeterminate:bg-accent",
          "outline-none focus-visible:ring-3 focus-visible:ring-accent-soft",
          "disabled:cursor-default disabled:opacity-45",
        )}
        {...props}
      />
      <Check
        size={10}
        strokeWidth={3}
        aria-hidden
        className="pointer-events-none absolute inset-0 m-auto text-accent-fg opacity-0 peer-checked:opacity-100 peer-indeterminate:opacity-0"
      />
      <Minus
        size={10}
        strokeWidth={3}
        aria-hidden
        className="pointer-events-none absolute inset-0 m-auto text-accent-fg opacity-0 peer-indeterminate:opacity-100"
      />
    </span>
  );
}

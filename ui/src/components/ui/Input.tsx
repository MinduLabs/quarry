import { forwardRef, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...props }, ref) {
    return (
      <input
        ref={ref}
        spellCheck={false}
        autoComplete="off"
        className={cn(
          "h-7 w-full rounded-md border border-line-strong bg-app px-2 text-base text-fg",
          "placeholder:text-faint transition-colors-fast",
          "focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-soft",
          "aria-invalid:border-danger",
          className,
        )}
        {...props}
      />
    );
  },
);

export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: the control is passed as children
    <label className={cn("flex flex-col gap-1", className)}>
      <span className="text-sm text-muted">{label}</span>
      {children}
      {hint && <span className="text-xs text-faint">{hint}</span>}
    </label>
  );
}

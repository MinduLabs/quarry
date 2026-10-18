import { Eye, EyeOff } from "lucide-react";
import { forwardRef, type InputHTMLAttributes, useState } from "react";
import { cn } from "@/lib/cn";
import { IconButton } from "./IconButton";

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  /** `lg` is taller and rounder; used for search boxes. */
  inputSize?: "md" | "lg";
}

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { className, inputSize = "md", ...props },
  ref,
) {
  return (
    <input
      ref={ref}
      spellCheck={false}
      autoComplete="off"
      className={cn(
        "w-full border border-line-strong bg-app px-2 text-base text-fg",
        inputSize === "lg" ? "h-8 rounded-lg" : "h-7 rounded-md",
        "placeholder:text-faint transition-colors-fast",
        "focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-soft",
        "aria-invalid:border-danger",
        className,
      )}
      {...props}
    />
  );
});

/** Password field with a button that shows what was typed (never a saved secret). */
export function PasswordInput({ className, ...props }: Omit<InputProps, "type">) {
  const [visible, setVisible] = useState(false);
  return (
    <div className="relative">
      <Input
        {...props}
        type={visible ? "text" : "password"}
        className={cn("pr-8", visible && "font-mono", className)}
      />
      <IconButton
        label={visible ? "Hide password" : "Show password"}
        size="sm"
        aria-pressed={visible}
        onClick={() => setVisible((v) => !v)}
        className="absolute top-0.5 right-0.5"
      >
        {visible ? <EyeOff size={14} strokeWidth={1.75} /> : <Eye size={14} strokeWidth={1.75} />}
      </IconButton>
    </div>
  );
}

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

import { Search } from "lucide-react";
import { cn } from "@/lib/cn";

/**
 * A filter box at the top of a list or dialog. Focus only darkens the border: the box is where
 * typing goes anyway, so an accent ring would only shout. Escape clears it first; a second Escape
 * reaches the dialog and closes it.
 */
export function SearchInput({
  value,
  onChange,
  placeholder = "Search",
  label,
  autoFocus,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  /** Read by screen readers; the placeholder is not a label. */
  label: string;
  autoFocus?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("relative", className)}>
      <Search
        size={13}
        strokeWidth={1.75}
        className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-faint"
      />
      <input
        type="search"
        spellCheck={false}
        autoComplete="off"
        // biome-ignore lint/a11y/noAutofocus: a search box opens with its dialog to be typed in
        autoFocus={autoFocus}
        aria-label={label}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.preventDefault();
            e.stopPropagation();
            onChange("");
          }
        }}
        className={cn(
          "h-7 w-full rounded-md border border-line bg-surface pr-2 pl-7 text-base text-fg",
          "transition-colors-fast placeholder:text-faint",
          "outline-none hover:border-line-strong focus:border-line-strong focus:bg-app",
          "[&::-webkit-search-cancel-button]:hidden",
        )}
      />
    </div>
  );
}

import { cn } from "@/lib/cn";

/**
 * A keyboard shortcut drawn as key caps. `keys` is a space-separated chord, e.g. "⌘ N" or "Ctrl ⇧ ↵";
 * each part becomes its own key.
 */
export function Kbd({ keys, className }: { keys: string; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1", className)}>
      {keys.split(" ").map((key, index) => (
        <kbd
          // biome-ignore lint/suspicious/noArrayIndexKey: a chord may repeat a key
          key={`${key}${index}`}
          className={cn(
            "inline-flex h-5 min-w-5 items-center justify-center rounded-[5px] border border-line-strong",
            "border-b-2 bg-surface px-1 font-sans font-medium text-muted text-xs leading-none",
          )}
        >
          {key}
        </kbd>
      ))}
    </span>
  );
}

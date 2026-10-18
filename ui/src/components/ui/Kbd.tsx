import { cn } from "@/lib/cn";
import { keyCaps } from "@/lib/shortcuts";

/**
 * A keyboard shortcut drawn as key caps, one per key, in the platform's own names: `⌘ ⇧ T` on
 * macOS, `Ctrl Shift T` elsewhere. `keys` is platform-neutral (`["Mod", "Shift", "T"]`); take it
 * from `lib/shortcuts.ts`.
 */
export function Kbd({ keys, className }: { keys: readonly string[]; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-[3px]", className)}>
      {keyCaps(keys).map((key, index) => (
        <kbd
          // biome-ignore lint/suspicious/noArrayIndexKey: a chord may repeat a key
          key={`${key}${index}`}
          className={cn(
            "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-[4px] px-[5px]",
            "border border-kbd-border border-b-kbd-edge bg-kbd text-kbd-fg",
            "font-sans font-medium text-[11px] leading-none shadow-[0_1px_0_var(--kbd-edge)]",
          )}
        >
          {key}
        </kbd>
      ))}
    </span>
  );
}

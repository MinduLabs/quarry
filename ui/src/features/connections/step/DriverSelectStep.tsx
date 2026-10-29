import type { DataSource } from "@/bindings/DataSource";
import type { DataSourceSection } from "@/bindings/DataSourceSection";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { DataSourceLogo } from "../DataSourceLogo";

const SECTIONS: { value: DataSourceSection; label: string }[] = [
  { value: "database", label: "Data sources" },
  { value: "cloud", label: "Cloud providers" },
  { value: "demo", label: "Demo databases" },
];

/** Step 1: a click on a source opens its settings. Sources without a driver show but cannot be picked. */
export function DriverSelectStep({
  sources,
  selectedId,
  onPick,
}: {
  sources: DataSource[];
  /** The source picked before, marked when the user comes back to this step. */
  selectedId: string | null;
  onPick: (id: string) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      {SECTIONS.map((section) => {
        const items = sources.filter((s) => s.section === section.value);
        if (items.length === 0) return null;
        return (
          <section key={section.value} className="flex flex-col gap-2">
            <h3 className="font-medium text-muted text-sm">{section.label}</h3>
            <div className="grid grid-cols-[repeat(auto-fill,minmax(112px,1fr))] gap-3">
              {items.map((source) => (
                <SourceTile
                  key={source.id}
                  source={source}
                  isSelected={source.id === selectedId}
                  onPick={onPick}
                />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function SourceTile({
  source,
  isSelected,
  onPick,
}: {
  source: DataSource;
  isSelected: boolean;
  onPick: (id: string) => void;
}) {
  const isAvailable = source.driver !== null;
  const tile = (
    <button
      type="button"
      // aria-disabled, not disabled: a disabled button gets no hover, so no tooltip.
      aria-disabled={!isAvailable}
      aria-pressed={isSelected}
      onClick={() => isAvailable && onPick(source.id)}
      className={cn(
        "flex aspect-square min-w-0 flex-col items-center justify-center gap-3 rounded-lg border p-3",
        "transition-colors-fast outline-none focus-visible:ring-3 focus-visible:ring-accent-soft",
        isSelected ? "border-accent bg-selected" : "border-line",
        isAvailable ? "hover:bg-hover" : "cursor-not-allowed opacity-45",
      )}
    >
      <DataSourceLogo id={source.id} />
      <span className="line-clamp-2 text-center text-base text-fg leading-tight">
        {source.name}
      </span>
    </button>
  );
  if (isAvailable) return tile;
  return <Tooltip content="Coming soon">{tile}</Tooltip>;
}

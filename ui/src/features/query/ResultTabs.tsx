import { SquareTerminal, Table2, X } from "lucide-react";
import { forwardRef, type HTMLAttributes, type ReactNode } from "react";
import { ContextMenuArea } from "@/components/ui/Menu";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { type ResultSet, type Tab, useApp } from "@/store/app";

/**
 * The tabs above results: the output of the tab's session first (in the results panel), then
 * one per result set, named after the table it comes from. A result tab can be closed until the
 * next run.
 */
export function ResultTabs({
  tab,
  grids,
  label,
  output,
  actions,
}: {
  tab: Tab;
  grids: { set: ResultSet; index: number }[];
  /** The fallback name of the `n`th result, when its rows come from no single table. */
  label: (n: number) => string;
  /** The output tab, in the results panel: whether it is the one shown. */
  output?: { active: boolean };
  /** Buttons at the right end of the bar. */
  actions?: ReactNode;
}) {
  const { setActiveSet, closeSet, setPanelOutput } = useApp.getState();
  const outputShown = output?.active ?? false;
  const shown = grids.filter(({ index }) => !tab.closedSets.includes(index));
  const showResult = (index: number) => {
    setPanelOutput(false);
    setActiveSet(tab.id, index);
  };

  return (
    <div className="flex h-8 shrink-0 items-end border-line border-b bg-surface">
      <div
        role="tablist"
        aria-label="Results"
        className="flex min-w-0 flex-1 items-end gap-0.5 overflow-x-auto px-1"
      >
        {output && (
          <ResultTab
            active={outputShown}
            onSelect={() => setPanelOutput(true)}
            icon={<SquareTerminal size={13} strokeWidth={1.75} />}
            title="Output"
          />
        )}
        {output && shown.length === 0 && (tab.running || tab.error || tab.sets.length > 0) && (
          <ResultTab
            active={!outputShown}
            onSelect={() => setPanelOutput(false)}
            icon={tab.running ? <Spinner size={11} /> : <Table2 size={13} strokeWidth={1.75} />}
            title="Result"
          />
        )}
        {shown.map(({ set, index }) => {
          const n = grids.findIndex((g) => g.index === index);
          const others = shown.map((g) => g.index).filter((i) => i !== index);
          return (
            <ContextMenuArea
              key={index}
              items={[
                { label: "Close", onSelect: () => closeSet(tab.id, index) },
                {
                  label: "Close others",
                  disabled: others.length === 0,
                  onSelect: () => {
                    showResult(index);
                    for (const other of others) closeSet(tab.id, other);
                  },
                },
              ]}
            >
              <ResultTab
                active={!outputShown && tab.activeSet === index}
                onSelect={() => showResult(index)}
                onClose={() => closeSet(tab.id, index)}
                icon={<Table2 size={13} strokeWidth={1.75} />}
                title={sourceTable(set) ?? label(n)}
              />
            </ContextMenuArea>
          );
        })}
      </div>
      {actions && (
        <div className="flex shrink-0 items-center gap-0.5 self-center px-1">{actions}</div>
      )}
    </div>
  );
}

/** One tab of the bar. Takes a ref and passes other props on, so a menu can wrap it. */
const ResultTab = forwardRef<
  HTMLDivElement,
  {
    active: boolean;
    onSelect: () => void;
    onClose?: () => void;
    icon: ReactNode;
    title: string;
  } & Omit<HTMLAttributes<HTMLDivElement>, "title">
>(function ResultTab({ active, onSelect, onClose, icon, title, className, ...props }, ref) {
  return (
    <div
      ref={ref}
      {...props}
      role="tab"
      tabIndex={0}
      aria-selected={active}
      title={title}
      onMouseDown={(e) => {
        if (e.button === 1 && onClose) onClose();
      }}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onSelect();
      }}
      className={cn(
        "group relative flex h-7 max-w-60 shrink-0 items-center gap-1.5 rounded-t-md px-2.5 text-base transition-colors-fast",
        "focus-visible:outline-2 focus-visible:outline-accent",
        active ? "bg-app text-fg" : "text-muted hover:bg-hover hover:text-fg",
        className,
      )}
    >
      {active && <span aria-hidden className="absolute inset-x-0 bottom-0 h-0.5 bg-accent" />}
      <span className="shrink-0">{icon}</span>
      <span className="min-w-0 truncate">{title}</span>
      {onClose && (
        <button
          type="button"
          aria-label={`Close ${title}`}
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          className={cn(
            "-mr-1 flex size-5 items-center justify-center rounded-sm text-muted hover:bg-pressed hover:text-fg",
            !active && "opacity-0 group-hover:opacity-100",
          )}
        >
          <X size={12} strokeWidth={2} />
        </button>
      )}
    </div>
  );
});

/** `database.table` when every column with a source comes from the same table. */
function sourceTable(set: ResultSet): string | null {
  let found: string | null = null;
  for (const column of set.columns ?? []) {
    if (!column.source_table) continue;
    const name = column.source_database
      ? `${column.source_database}.${column.source_table}`
      : column.source_table;
    if (found !== null && found !== name) return null;
    found = name;
  }
  return found;
}

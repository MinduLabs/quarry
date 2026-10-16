import { Plus, X } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { modKey } from "@/lib/platform";
import { useApp } from "@/store/app";

export function TabBar() {
  const tabs = useApp((s) => s.tabs);
  const activeId = useApp((s) => s.activeTabId);
  const { selectTab, closeTab, newQuery } = useApp.getState();

  return (
    <div
      data-tauri-drag-region
      role="tablist"
      className="flex h-9 shrink-0 items-center gap-1 border-line border-b bg-surface px-2"
    >
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-selected={active}
            title={tab.title}
            onMouseDown={(e) => {
              if (e.button === 1) closeTab(tab.id);
              else selectTab(tab.id);
            }}
            className={cn(
              "animate-pop group flex h-7 max-w-55 min-w-24 items-center gap-1.5 rounded-md pr-1 pl-3 text-base transition-colors-fast",
              active
                ? "bg-app text-fg shadow-[0_0_0_1px_var(--line)]"
                : "text-muted hover:bg-hover hover:text-fg",
            )}
          >
            <span className="min-w-0 flex-1 truncate">{tab.title}</span>
            {tab.running ? (
              <span className="flex size-5 items-center justify-center">
                <Spinner size={11} />
              </span>
            ) : (
              <button
                type="button"
                aria-label={`Close ${tab.title}`}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => closeTab(tab.id)}
                className={cn(
                  "flex size-5 items-center justify-center rounded-sm text-muted hover:bg-pressed hover:text-fg",
                  !active && "opacity-0 group-hover:opacity-100",
                )}
              >
                <X size={12} strokeWidth={2} />
              </button>
            )}
          </div>
        );
      })}
      <IconButton label="New query" shortcut={`${modKey} T`} size="sm" onClick={() => newQuery()}>
        <Plus size={14} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}

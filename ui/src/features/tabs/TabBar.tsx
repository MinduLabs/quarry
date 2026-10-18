import { Plus, X } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { ContextMenuArea, type MenuItem } from "@/components/ui/Menu";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { isMac } from "@/lib/platform";
import { keysOf } from "@/lib/shortcuts";
import { useApp } from "@/store/app";

export function TabBar() {
  const tabs = useApp((s) => s.tabs);
  const activeId = useApp((s) => s.activeTabId);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const canReopen = useApp((s) => s.closedTabs.length > 0);
  const { selectTab, closeTab, closeTabs, reopenClosedTab, newQuery } = useApp.getState();

  const menu = (id: number): MenuItem[] => {
    const others = tabs.filter((t) => t.id !== id).map((t) => t.id);
    return [
      {
        label: "Reopen closed tab",
        shortcut: keysOf("reopenTab"),
        disabled: !canReopen,
        onSelect: reopenClosedTab,
      },
      {
        label: "Close",
        shortcut: keysOf("closeTab"),
        onSelect: () => closeTab(id),
        separatorBefore: true,
      },
      {
        label: "Close other tabs",
        disabled: others.length === 0,
        onSelect: () => closeTabs(others),
      },
    ];
  };

  return (
    <div
      data-tauri-drag-region
      role="tablist"
      className={cn(
        // Same height as the sidebar's top row.
        "flex h-9 shrink-0 items-center gap-1 border-line border-b bg-surface",
        // Hidden sidebar: the window buttons (macOS) and the toggle now sit over the tab bar.
        sidebarOpen ? "pl-2" : isMac ? "pl-32" : "pl-12",
        // Leave room for the window controls drawn over the right edge.
        isMac ? "pr-2" : "pr-36",
      )}
    >
      {tabs.map((tab) => {
        const active = tab.id === activeId;
        return (
          <ContextMenuArea key={tab.id} items={menu(tab.id)}>
            <div
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
              <button
                type="button"
                aria-label={`Close ${tab.title}`}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => closeTab(tab.id)}
                className={cn(
                  "group/close flex size-5 items-center justify-center rounded-sm text-muted hover:bg-pressed hover:text-fg",
                  !active && !tab.running && "opacity-0 group-hover:opacity-100",
                )}
              >
                {/* A running tab shows its spinner until hovered, so it can still be closed. */}
                {tab.running && (
                  <span className="group-hover/close:hidden group-focus-visible/close:hidden">
                    <Spinner size={11} />
                  </span>
                )}
                <X
                  size={12}
                  strokeWidth={2}
                  className={cn(
                    tab.running && "hidden group-hover/close:block group-focus-visible/close:block",
                  )}
                />
              </button>
            </div>
          </ContextMenuArea>
        );
      })}
      <IconButton
        label="New query"
        shortcut={keysOf("newQuery")}
        size="sm"
        onClick={() => newQuery()}
      >
        <Plus size={14} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}

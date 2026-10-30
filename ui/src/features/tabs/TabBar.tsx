import { ChevronDown, Pin, Plus, X } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { IconButton } from "@/components/ui/IconButton";
import { ContextMenuArea, DropdownMenuButton, type MenuItem } from "@/components/ui/Menu";
import { Spinner } from "@/components/ui/Spinner";
import { cn } from "@/lib/cn";
import { isMac } from "@/lib/platform";
import { keysOf } from "@/lib/shortcuts";
import { type TabGroup, useApp } from "@/store/app";

/** Pointer movement before a press on a tab becomes a drag, so a click never reorders. */
const DRAG_THRESHOLD = 6;

/** Reorder by dragging within the tab's group: the tab follows the pointer across its
 * neighbours. */
function startDrag(id: number, group: TabGroup, startX: number) {
  let isDragging = false;
  const onMove = (e: PointerEvent) => {
    if (!isDragging && Math.abs(e.clientX - startX) < DRAG_THRESHOLD) return;
    isDragging = true;
    const tabs = [...document.querySelectorAll<HTMLElement>(`[data-tab-group="${group}"]`)];
    const over = tabs.find((el) => {
      const r = el.getBoundingClientRect();
      return e.clientX >= r.left && e.clientX < r.right;
    });
    const overId = Number(over?.dataset.tabId);
    const index = useApp.getState().tabs.findIndex((t) => t.id === overId);
    if (index >= 0) useApp.getState().moveTab(id, index);
  };
  const stop = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", stop);
    window.removeEventListener("pointercancel", stop);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", stop);
  window.addEventListener("pointercancel", stop);
}

/**
 * The tabs of one group. `chromeLeft` and `chromeRight` leave room for what the window draws
 * over the bar's ends (window buttons, the sidebar toggle).
 */
export function TabBar({
  group,
  chromeLeft,
  chromeRight,
}: {
  group: TabGroup;
  chromeLeft: boolean;
  chromeRight: boolean;
}) {
  const allTabs = useApp((s) => s.tabs);
  const tabs = useMemo(() => allTabs.filter((t) => t.group === group), [allTabs, group]);
  const isFocused = useApp((s) => s.focusedGroup === group);
  const activeId = useApp((s) => (s.focusedGroup === group ? s.activeTabId : s.otherActiveTabId));
  const split = useApp((s) => s.split);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const canReopen = useApp((s) => s.closedTabs.length > 0);
  const connections = useApp((s) => s.connections);
  const {
    selectTab,
    closeTab,
    closeTabs,
    reopenClosedTab,
    newQuery,
    togglePin,
    splitTab,
    moveToOtherGroup,
    unsplit,
    focusGroup,
  } = useApp.getState();

  const strip = useRef<HTMLDivElement>(null);
  // A tab picked from elsewhere (the sidebar, a shortcut, a new tab) may be scrolled out of view.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `tabs.length` re-runs it for a new tab
  useEffect(() => {
    strip.current
      ?.querySelector(`[data-tab-id="${activeId}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeId, tabs.length]);

  const menu = (id: number): MenuItem[] => {
    const others = tabs.filter((t) => t.id !== id && !t.pinned).map((t) => t.id);
    return [
      {
        label: tabs.find((t) => t.id === id)?.pinned ? "Unpin tab" : "Pin tab",
        onSelect: () => togglePin(id),
      },
      ...(split
        ? [
            {
              label: "Move to other group",
              onSelect: () => moveToOtherGroup(id),
              separatorBefore: true,
            },
            {
              label: split === "right" ? "Split down instead" : "Split right instead",
              onSelect: () => splitTab(id, split === "right" ? "down" : "right"),
            },
            { label: "Unsplit", onSelect: unsplit },
          ]
        : [
            {
              label: "Split right",
              disabled: allTabs.length < 2,
              onSelect: () => splitTab(id, "right"),
              separatorBefore: true,
            },
            {
              label: "Split down",
              disabled: allTabs.length < 2,
              onSelect: () => splitTab(id, "down"),
            },
          ]),
      {
        label: "Reopen closed tab",
        disabled: !canReopen,
        onSelect: reopenClosedTab,
        separatorBefore: true,
      },
      {
        label: "Close",
        onSelect: () => closeTab(id),
        separatorBefore: true,
      },
      {
        label: "Close other tabs",
        disabled: others.length === 0,
        onSelect: () => closeTabs(others),
      },
      {
        label: "Close tabs to the right",
        disabled: !tabs.slice(tabs.findIndex((t) => t.id === id) + 1).some((t) => !t.pinned),
        onSelect: () => useApp.getState().closeTabsToRight(id),
      },
    ];
  };

  return (
    <div
      data-tauri-drag-region
      role="tablist"
      className={cn(
        // Same height as the sidebar's top row.
        "flex h-9 min-w-0 shrink-0 items-center gap-1 border-line border-b bg-surface",
        // Hidden sidebar: the window buttons (macOS) and the toggle now sit over the tab bar.
        !chromeLeft || sidebarOpen ? "pl-2" : isMac ? "pl-32" : "pl-12",
        // Leave room for the window controls drawn over the right edge.
        chromeRight && !isMac ? "pr-36" : "pr-2",
      )}
    >
      <div
        ref={strip}
        // A mouse wheel scrolls the strip sideways; a trackpad already does. No scrollbar: the
        // bar is 36px tall and the active tab is always scrolled into view.
        onWheel={(e) => {
          if (e.deltaX === 0 && e.deltaY !== 0) e.currentTarget.scrollLeft += e.deltaY;
        }}
        className="flex min-w-0 items-center gap-1 overflow-x-auto [&::-webkit-scrollbar]:hidden [scrollbar-width:none]"
      >
        {tabs.map((tab) => {
          const active = tab.id === activeId;
          const environment = connections.find((c) => c.id === tab.connId)?.environment;
          const env = environment === "local" ? undefined : environment;
          // The environment shows as a badge in the tab's toolbar; tabs stay neutral and name it on
          // hover.
          return (
            <ContextMenuArea key={tab.id} items={menu(tab.id)}>
              <div
                role="tab"
                data-tab-id={tab.id}
                data-tab-group={group}
                aria-selected={active}
                title={
                  env
                    ? `${tab.title} · ${env === "production" ? "Production" : "Staging"}`
                    : tab.title
                }
                onMouseDown={(e) => {
                  if (e.button === 1) {
                    if (!tab.pinned) closeTab(tab.id);
                    return;
                  }
                  selectTab(tab.id);
                  if (e.button === 0) startDrag(tab.id, group, e.clientX);
                }}
                className={cn(
                  "animate-pop group flex h-7 shrink-0 items-center gap-1.5 rounded-lg text-base transition-colors-fast",
                  tab.pinned ? "max-w-32 min-w-0 pr-3 pl-2.5" : "max-w-55 min-w-24 pr-1 pl-3",
                  // The active tab is a filled shape with no outline, like a browser's: a hairline
                  // ring drew a lighter edge into the rounded corners.
                  active
                    ? cn(
                        "bg-pressed",
                        // In a split, the focused group's tab is the one the keyboard acts on.
                        isFocused ? "text-fg" : "text-muted",
                      )
                    : "text-muted hover:bg-hover hover:text-fg",
                )}
              >
                {tab.pinned && <Pin size={12} strokeWidth={1.75} className="shrink-0" />}
                <span className="min-w-0 flex-1 truncate">{tab.title}</span>
                {!tab.pinned && (
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
                        tab.running &&
                          "hidden group-hover/close:block group-focus-visible/close:block",
                      )}
                    />
                  </button>
                )}
              </div>
            </ContextMenuArea>
          );
        })}
      </div>
      <IconButton
        label="New query"
        shortcut={keysOf("newQuery")}
        size="sm"
        onClick={() => {
          focusGroup(group);
          newQuery();
        }}
      >
        <Plus size={14} strokeWidth={1.75} />
      </IconButton>
      {tabs.length > 0 && (
        <DropdownMenuButton
          // Every tab of the group, also the ones scrolled out of view; picking one scrolls to it.
          items={tabs.map((tab) => ({
            id: String(tab.id),
            label: tab.title,
            onSelect: () => selectTab(tab.id),
          }))}
          trigger={
            <IconButton label="All tabs" size="sm">
              <ChevronDown size={14} strokeWidth={1.75} />
            </IconButton>
          }
        />
      )}
    </div>
  );
}

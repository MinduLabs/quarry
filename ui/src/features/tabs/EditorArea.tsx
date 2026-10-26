import { Profiler, useRef, useState } from "react";
import { cn } from "@/lib/cn";
import { onRender } from "@/lib/perf";
import { type TabGroup, useApp } from "@/store/app";
import { QueryView } from "../query/QueryView";
import { TabBar } from "./TabBar";

/** The smallest share of the area a group keeps while the divider is dragged. */
const MIN_SHARE = 0.2;

/**
 * The open tabs: one group, or two side by side (split right) or one above the other (split
 * down), each with its own tab bar. A press inside a group focuses it.
 */
export function EditorArea() {
  const split = useApp((s) => s.split);
  const [share, setShare] = useState(0.5);
  const area = useRef<HTMLDivElement>(null);

  if (!split) return <Group group={0} chromeLeft chromeRight />;

  const isRow = split === "right";
  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    const box = area.current?.getBoundingClientRect();
    if (!box) return;
    const move = (e: PointerEvent) => {
      const at = isRow ? (e.clientX - box.left) / box.width : (e.clientY - box.top) / box.height;
      setShare(Math.min(1 - MIN_SHARE, Math.max(MIN_SHARE, at)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <div ref={area} className={cn("flex min-h-0 flex-1", isRow ? "flex-row" : "flex-col")}>
      <div style={{ flexBasis: `${share * 100}%` }} className="flex min-h-0 min-w-0 shrink-0">
        <Group group={0} chromeLeft chromeRight={!isRow} />
      </div>
      <div
        role="separator"
        aria-orientation={isRow ? "vertical" : "horizontal"}
        onPointerDown={startResize}
        className={cn(
          "group relative shrink-0 bg-line",
          isRow ? "w-px cursor-col-resize" : "h-px cursor-row-resize",
        )}
      >
        <div
          className={cn(
            "absolute transition-colors-fast group-hover:bg-accent-soft",
            isRow ? "inset-y-0 -left-1 w-2" : "inset-x-0 -top-1 h-2",
          )}
        />
      </div>
      <div className="flex min-h-0 min-w-0 flex-1">
        <Group group={1} chromeLeft={false} chromeRight />
      </div>
    </div>
  );
}

function Group({
  group,
  chromeLeft,
  chromeRight,
}: {
  group: TabGroup;
  chromeLeft: boolean;
  chromeRight: boolean;
}) {
  const tabs = useApp((s) => s.tabs);
  const shownId = useApp((s) => (s.focusedGroup === group ? s.activeTabId : s.otherActiveTabId));
  return (
    <section
      aria-label={group === 0 ? "Tabs" : "Second tab group"}
      className="flex min-h-0 min-w-0 flex-1 flex-col"
      onPointerDownCapture={() => useApp.getState().focusGroup(group)}
    >
      <TabBar group={group} chromeLeft={chromeLeft} chromeRight={chromeRight} />
      {tabs.map((tab) =>
        tab.group === group ? (
          // Keep every tab mounted so editors keep their state; hide the others.
          <div
            key={tab.id}
            className={tab.id === shownId ? "flex min-h-0 flex-1 flex-col" : "hidden"}
          >
            <Profiler id="tab" onRender={onRender}>
              <QueryView tab={tab} />
            </Profiler>
          </div>
        ) : null,
      )}
    </section>
  );
}

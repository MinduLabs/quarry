import { ChevronDown, ChevronUp, PanelTop, SquareTerminal } from "lucide-react";
import { useState } from "react";
import { IconButton } from "@/components/ui/IconButton";
import { useApp } from "@/store/app";
import { ConsoleView } from "../console/ConsoleView";
import { ResultsPane } from "./ResultsPane";

const MIN_HEIGHT = 120;
/** Room the tabs above keep. */
const EDITOR_MIN = 160;

/**
 * The panel at the bottom of the window, always there like a terminal: the output of the
 * selected tab, and the results of a query tab beside it. A table tab shows its rows in the tab
 * itself, so here it has its output only.
 */
export function ResultsPanel() {
  const tab = useApp((s) => s.tabs.find((t) => t.id === s.activeTabId));
  const isOpen = useApp((s) => s.panelOpen);
  const [height, setHeight] = useState(260);
  const { setPanelOpen, setInlineResults } = useApp.getState();

  if (!tab) return null;
  const hasResults = !tab.browse && !tab.inlineResults;

  if (!isOpen) {
    return (
      <div className="flex h-8 shrink-0 items-center gap-2 border-line border-t bg-surface px-3 text-muted text-sm">
        <SquareTerminal size={13} strokeWidth={1.75} />
        <span className="truncate">
          Output{hasResults ? " and results" : ""} · {tab.title}
        </span>
        <div className="flex-1" />
        <IconButton label="Show the panel" size="sm" onClick={() => setPanelOpen(true)}>
          <ChevronUp size={14} strokeWidth={1.75} />
        </IconButton>
      </div>
    );
  }

  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    const startY = event.clientY;
    const start = height;
    const move = (e: PointerEvent) =>
      setHeight(
        Math.max(
          MIN_HEIGHT,
          Math.min(window.innerHeight - EDITOR_MIN, start - (e.clientY - startY)),
        ),
      );
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const actions = (
    <>
      <span className="mr-1 max-w-40 truncate text-faint text-sm">{tab.title}</span>
      {hasResults && (
        <IconButton
          label="Show results under the editor"
          size="sm"
          onClick={() => setInlineResults(tab.id, true)}
        >
          <PanelTop size={14} strokeWidth={1.75} />
        </IconButton>
      )}
      <IconButton label="Hide the panel" size="sm" onClick={() => setPanelOpen(false)}>
        <ChevronDown size={14} strokeWidth={1.75} />
      </IconButton>
    </>
  );

  return (
    <section aria-label="Output and results" style={{ height }} className="flex shrink-0 flex-col">
      <div
        role="separator"
        aria-orientation="horizontal"
        onPointerDown={startResize}
        className="group relative h-px shrink-0 cursor-row-resize bg-line"
      >
        <div className="absolute inset-x-0 -top-1 h-2 transition-colors-fast group-hover:bg-accent-soft" />
      </div>
      <div className="min-h-0 flex-1">
        {hasResults ? (
          <ResultsPane tab={tab} withOutput actions={actions} />
        ) : (
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex h-8 shrink-0 items-end border-line border-b bg-surface">
              <div role="tablist" aria-label="Results" className="flex flex-1 items-end px-1">
                <div
                  role="tab"
                  aria-selected
                  className="relative flex h-7 items-center gap-1.5 rounded-t-md bg-app px-2.5 text-base text-fg"
                >
                  <span aria-hidden className="absolute inset-x-0 bottom-0 h-0.5 bg-accent" />
                  <SquareTerminal size={13} strokeWidth={1.75} />
                  Output
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-0.5 self-center px-1">{actions}</div>
            </div>
            <div className="min-h-0 flex-1">
              <ConsoleView tab={tab.id} log={tab.log} />
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

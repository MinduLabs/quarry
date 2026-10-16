import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/Toaster";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { send, subscribe } from "@/lib/engine";
import { useApp } from "@/store/app";
import { ConnectionDialog, DeleteConnectionDialog } from "./features/connections/ConnectionDialog";
import { ConfirmRunDialog } from "./features/query/ConfirmRunDialog";
import { QueryView } from "./features/query/QueryView";
import { Sidebar } from "./features/sidebar/Sidebar";
import { TabBar } from "./features/tabs/TabBar";
import { Welcome } from "./features/welcome/Welcome";

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 400;

export function App() {
  const tabs = useApp((s) => s.tabs);
  const activeId = useApp((s) => s.activeTabId);
  const [sidebarWidth, setSidebarWidth] = useState(248);

  useEffect(() => {
    subscribe(useApp.getState().handleEvent)
      .then(() => {
        send({ type: "Ping", id: 1 });
        send({ type: "LoadConnections" });
      })
      .catch((err: unknown) =>
        useApp.getState().toast("error", `Engine unavailable: ${String(err)}`),
      );
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const { openDialog, newQuery, closeTab, activeTabId, dialog } = useApp.getState();
      if (dialog) return;
      const key = e.key.toLowerCase();
      if (key === "n") openDialog({ mode: "new" });
      else if (key === "t") newQuery();
      else if (key === "w" && activeTabId !== null) closeTab(activeTabId);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const startResize = (event: React.PointerEvent) => {
    event.preventDefault();
    const move = (e: PointerEvent) =>
      setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, e.clientX)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <TooltipProvider>
      <div className="flex h-full">
        <div style={{ width: sidebarWidth }} className="shrink-0">
          <Sidebar />
        </div>
        <div
          role="separator"
          aria-orientation="vertical"
          onPointerDown={startResize}
          className="relative w-px shrink-0 cursor-col-resize bg-line"
        >
          <div className="absolute inset-y-0 -left-1 w-2 transition-colors-fast hover:bg-accent-soft" />
        </div>
        <main className="flex min-w-0 flex-1 flex-col bg-app">
          {tabs.length === 0 ? (
            <Welcome />
          ) : (
            <>
              <TabBar />
              {tabs.map((tab) => (
                // Keep every tab mounted so editors keep their state; hide inactive ones.
                <div
                  key={tab.id}
                  className={tab.id === activeId ? "flex min-h-0 flex-1 flex-col" : "hidden"}
                >
                  <QueryView tab={tab} />
                </div>
              ))}
            </>
          )}
        </main>
      </div>
      <ConnectionDialog />
      <DeleteConnectionDialog />
      <ConfirmRunDialog />
      <Toaster />
    </TooltipProvider>
  );
}

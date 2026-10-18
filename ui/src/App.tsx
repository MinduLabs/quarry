import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { Toaster } from "@/components/ui/Toaster";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { send, subscribe } from "@/lib/engine";
import { isMac } from "@/lib/platform";
import { pendingWork, saveWorkspace, useApp } from "@/store/app";
import { ConnectionDialog, DeleteConnectionDialog } from "./features/connections/ConnectionDialog";
import { ConfirmChangesDialog } from "./features/query/ConfirmChangesDialog";
import { ConfirmRunDialog } from "./features/query/ConfirmRunDialog";
import { QueryView } from "./features/query/QueryView";
import { SettingsDialog } from "./features/settings/SettingsDialog";
import { Sidebar } from "./features/sidebar/Sidebar";
import { SidebarToggle } from "./features/sidebar/SidebarToggle";
import { CloseTabDialog } from "./features/tabs/CloseTabDialog";
import { TabBar } from "./features/tabs/TabBar";
import { Welcome } from "./features/welcome/Welcome";
import { QuitDialog } from "./features/window/QuitDialog";
import { WindowControls } from "./features/window/WindowControls";

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 400;

export function App() {
  const tabs = useApp((s) => s.tabs);
  const activeId = useApp((s) => s.activeTabId);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
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

  // Closing the window (button, Alt+F4, the macOS red button) asks first only if a query is
  // running. Otherwise it saves the open tabs, then closes.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    const unlisten = appWindow.onCloseRequested((event) => {
      event.preventDefault();
      const work = pendingWork(useApp.getState().tabs);
      if (work) useApp.getState().askQuit(work);
      else void saveWorkspace().then(() => appWindow.destroy());
    });
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  // The webview's own right-click menu offers Reload, Back and Inspect, which restart the whole
  // UI. Keep it only where it helps: text fields and the editor (cut, copy, paste). In
  // development, Shift + right-click still opens it for the inspector.
  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      if (import.meta.env.DEV && e.shiftKey) return;
      const target = e.target instanceof Element ? e.target : null;
      if (target?.closest("input, textarea, [contenteditable='true']")) return;
      e.preventDefault();
    };
    // The webview would reload the whole UI, dropping running queries and unsaved edits.
    // Instead, F5 and Ctrl/⌘+R refresh the active tab's results.
    const onReloadKey = (e: KeyboardEvent) => {
      const reload = e.key === "F5" || ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r");
      if (!reload) return;
      e.preventDefault();
      // Shift reloads the webview, kept for development only.
      if (e.shiftKey) {
        if (import.meta.env.DEV) window.location.reload();
        return;
      }
      const { activeTabId, dialog, refresh } = useApp.getState();
      if (activeTabId !== null && !dialog) refresh(activeTabId);
    };
    window.addEventListener("contextmenu", onContextMenu);
    window.addEventListener("keydown", onReloadKey);
    return () => {
      window.removeEventListener("contextmenu", onContextMenu);
      window.removeEventListener("keydown", onReloadKey);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const { openDialog, newQuery, closeTab, activeTabId, dialog } = useApp.getState();
      if (dialog) return;
      const key = e.key.toLowerCase();
      if (key === "n") openDialog({ mode: "new" });
      else if (key === "t" && e.shiftKey) useApp.getState().reopenClosedTab();
      else if (key === "t") newQuery();
      else if (key === "w" && activeTabId !== null) closeTab(activeTabId);
      else if (key === "b") useApp.getState().toggleSidebar();
      else if (key === ",") useApp.getState().setSettingsOpen(true);
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
      <div className="relative flex h-full overflow-hidden">
        {/* The sidebar slides with a transform only (no layout animation); the content area
            changes size at once. It is inert while hidden so Tab can't reach it. */}
        <div
          inert={!sidebarOpen}
          style={{ width: sidebarWidth + 1 }}
          className={cn(
            "absolute inset-y-0 left-0 z-20 flex transition-transform duration-(--duration-slow) ease-(--ease-out)",
            sidebarOpen ? "translate-x-0" : "-translate-x-full",
          )}
        >
          <div className="min-w-0 flex-1">
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
        </div>
        <SidebarToggle />
        <main
          style={{ marginLeft: sidebarOpen ? sidebarWidth + 1 : 0 }}
          className="relative flex min-w-0 flex-1 flex-col bg-app"
        >
          {!isMac && (
            <div className="absolute top-0 right-0 z-30">
              <WindowControls />
            </div>
          )}
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
      <ConfirmChangesDialog />
      <SettingsDialog />
      <CloseTabDialog />
      <QuitDialog />
      <Toaster />
    </TooltipProvider>
  );
}

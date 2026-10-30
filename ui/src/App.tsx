import { getCurrentWindow } from "@tauri-apps/api/window";
import { Profiler, useEffect, useState } from "react";
import { ResizeHandle } from "@/components/ui/ResizeHandle";
import { Toaster } from "@/components/ui/Toaster";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { send, subscribe } from "@/lib/engine";
import { onRender } from "@/lib/perf";
import { isMac } from "@/lib/platform";
import { pendingWork, saveWorkspace, useApp } from "@/store/app";
import { ConnectionDialog, DeleteConnectionDialog } from "./features/connections/ConnectionDialog";
import { ExportDialog } from "./features/export/ExportDialog";
import { ExportProgress } from "./features/export/ExportProgress";
import { HistoryDialog } from "./features/history/HistoryDialog";
import { ImportDialog } from "./features/import/ImportDialog";
import { ImportProgress } from "./features/import/ImportProgress";
import { CommandPalette } from "./features/palette/CommandPalette";
import { ConfirmChangesDialog } from "./features/query/ConfirmChangesDialog";
import { ConfirmDeleteRowsDialog } from "./features/query/ConfirmDeleteRowsDialog";
import { ConfirmRunDialog } from "./features/query/ConfirmRunDialog";
import { ResultsPanel } from "./features/query/ResultsPanel";
import { SchemaFormDialog } from "./features/schema/SchemaFormDialog";
import { SchemaReviewDialog } from "./features/schema/SchemaReviewDialog";
import { ServerInfoDialog } from "./features/server/ServerInfoDialog";
import { SettingsDialog } from "./features/settings/SettingsDialog";
import { SchemaPickerDialog } from "./features/sidebar/SchemaPickerDialog";
import { Sidebar } from "./features/sidebar/Sidebar";
import { SidebarToggle } from "./features/sidebar/SidebarToggle";
import { CloseTabDialog } from "./features/tabs/CloseTabDialog";
import { EditorArea } from "./features/tabs/EditorArea";
import { Welcome } from "./features/welcome/Welcome";
import { QuitDialog } from "./features/window/QuitDialog";
import { WindowControls } from "./features/window/WindowControls";

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 400;

export function App() {
  const tabs = useApp((s) => s.tabs);
  const sidebarOpen = useApp((s) => s.sidebarOpen);
  const [sidebarWidth, setSidebarWidth] = useState(248);

  useEffect(() => {
    subscribe(useApp.getState().handleEvent)
      .then(() => {
        send({ type: "Ping", id: 1 });
        useApp.getState().checkForUpdate();
        send({ type: "LoadConnections" });
        send({ type: "LoadDataSources" });
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
      const { openDialog, newQuery, closeTab, activeTabId, dialog, tabs } = useApp.getState();
      if (dialog) return;
      const key = e.key.toLowerCase();
      // Ctrl+Tab on every platform: ⌘Tab belongs to the macOS app switcher.
      if (key === "tab" && e.ctrlKey) useApp.getState().cycleTab(e.shiftKey ? -1 : 1);
      else if (/^[1-9]$/.test(key) && !e.shiftKey) useApp.getState().selectTabAt(Number(key));
      else if (key === "n") openDialog({ mode: "new" });
      else if (key === "t" && e.shiftKey) useApp.getState().reopenClosedTab();
      else if (key === "t") newQuery();
      else if (key === "w" && activeTabId !== null) {
        // A pinned tab is closed on purpose only: from its menu.
        if (!tabs.find((t) => t.id === activeTabId)?.pinned) closeTab(activeTabId);
      } else if (key === "b") useApp.getState().toggleSidebar();
      else if (key === ",") useApp.getState().setSettingsOpen(true);
      else if (key === "k" && !e.shiftKey) useApp.getState().setPalette("open");
      else if (key === "p" && e.shiftKey) useApp.getState().setPalette("commands");
      else if (key === (isMac ? "y" : "h") && !e.shiftKey) useApp.getState().setHistoryOpen(true);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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
            <Profiler id="sidebar" onRender={onRender}>
              <Sidebar />
            </Profiler>
          </div>
          <ResizeHandle
            orientation="vertical"
            onStart={() => {
              const start = sidebarWidth;
              return (delta) =>
                setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, start + delta)));
            }}
          />
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
              <EditorArea />
              <ResultsPanel />
            </>
          )}
        </main>
      </div>
      <ConnectionDialog />
      <DeleteConnectionDialog />
      <ConfirmRunDialog />
      <ConfirmChangesDialog />
      <ConfirmDeleteRowsDialog />
      <SettingsDialog />
      <SchemaFormDialog />
      <SchemaReviewDialog />
      <ExportDialog />
      <ExportProgress />
      <ImportDialog />
      <ImportProgress />
      <CommandPalette />
      <HistoryDialog />
      <ServerInfoDialog />
      <SchemaPickerDialog />
      <CloseTabDialog />
      <QuitDialog />
      <Toaster />
    </TooltipProvider>
  );
}

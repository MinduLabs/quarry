import { PanelLeft } from "lucide-react";
import { IconButton } from "@/components/ui/IconButton";
import { cn } from "@/lib/cn";
import { isMac } from "@/lib/platform";
import { keysOf } from "@/lib/shortcuts";
import { useApp } from "@/store/app";

/**
 * Show / hide the sidebar. It lives at the window level, not inside the sidebar, so it stays put
 * (and reachable) while the sidebar is hidden. On macOS it sits 84px from the left edge, clear of the
 * window buttons (they end at about 70px).
 */
export function SidebarToggle() {
  const open = useApp((s) => s.sidebarOpen);
  const toggle = useApp((s) => s.toggleSidebar);
  return (
    <div className={cn("absolute z-30", isMac ? "top-1 left-21" : "top-1 left-2")}>
      <IconButton
        label={open ? "Hide sidebar" : "Show sidebar"}
        shortcut={keysOf("toggleSidebar")}
        aria-pressed={open}
        onClick={toggle}
      >
        <PanelLeft size={16} strokeWidth={1.75} />
      </IconButton>
    </div>
  );
}

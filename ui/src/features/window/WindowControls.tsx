import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { cn } from "@/lib/cn";

const ICON = { size: 14, strokeWidth: 1.5 } as const;

/**
 * Minimize / maximize / close for Linux, where the window has no native frame.
 * (macOS keeps its traffic lights; see tauri.conf.json.)
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    const sync = () => {
      win
        .isMaximized()
        .then(setMaximized)
        .catch(() => {});
    };
    sync();
    const unlisten = win.onResized(sync);
    return () => {
      unlisten.then((fn) => fn()).catch(() => {});
    };
  }, []);

  const win = getCurrentWindow();
  return (
    <div className="flex h-9 items-stretch">
      <Control label="Minimize" onClick={() => void win.minimize()}>
        <Minus {...ICON} />
      </Control>
      <Control label={maximized ? "Restore" : "Maximize"} onClick={() => void win.toggleMaximize()}>
        {maximized ? <Copy {...ICON} className="-scale-x-100" /> : <Square {...ICON} size={12} />}
      </Control>
      <Control label="Close" danger onClick={() => void win.close()}>
        <X {...ICON} />
      </Control>
    </div>
  );
}

function Control({
  label,
  danger,
  onClick,
  children,
}: {
  label: string;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "inline-flex w-11 items-center justify-center text-muted transition-colors-fast",
        danger
          ? "hover:bg-danger-solid hover:text-white"
          : "hover:bg-hover hover:text-fg active:bg-pressed",
      )}
    >
      {children}
    </button>
  );
}

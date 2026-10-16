import * as ContextMenu from "@radix-ui/react-context-menu";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  separatorBefore?: boolean;
}

/** Right-click menu around `children`. */
export function ContextMenuArea({ items, children }: { items: MenuItem[]; children: ReactNode }) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="animate-pop z-50 min-w-44 rounded-lg bg-elevated p-1 shadow-popover">
          {items.map((item) => (
            <div key={item.label}>
              {item.separatorBefore && <ContextMenu.Separator className="my-1 h-px bg-line" />}
              <ContextMenu.Item
                onSelect={item.onSelect}
                className={cn(
                  "flex h-7 cursor-default select-none items-center rounded-md px-2 text-base outline-none",
                  "data-highlighted:bg-accent data-highlighted:text-accent-fg",
                  item.danger ? "text-danger" : "text-fg",
                )}
              >
                {item.label}
              </ContextMenu.Item>
            </div>
          ))}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

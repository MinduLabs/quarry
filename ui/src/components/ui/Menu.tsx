import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  /** A line above this item; ignored on the first item, so optional items above it can go. */
  separatorBefore?: boolean;
  disabled?: boolean;
  /** A checkbox item (dropdown menus only): shows a check and keeps the menu open. */
  checked?: boolean;
}

const CONTENT = "animate-pop z-50 min-w-44 rounded-lg bg-elevated p-1 shadow-popover";

const itemClass = (item: MenuItem) =>
  cn(
    "flex h-7 cursor-default select-none items-center rounded-md px-2 text-base outline-none",
    "data-highlighted:bg-accent data-highlighted:text-accent-fg data-disabled:opacity-45",
    item.danger ? "text-danger" : "text-fg",
  );

/** Right-click menu around `children`. Items are names only: shortcuts live in tooltips, the
 * command palette and Settings › Keyboard, never in a menu. */
export function ContextMenuArea({ items, children }: { items: MenuItem[]; children: ReactNode }) {
  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={CONTENT}>
          {items.map((item, index) => (
            <div key={item.label}>
              {item.separatorBefore && index > 0 && (
                <ContextMenu.Separator className="my-1 h-px bg-line" />
              )}
              <ContextMenu.Item
                onSelect={item.onSelect}
                disabled={item.disabled}
                className={itemClass(item)}
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

/**
 * A menu opened by a button (`trigger`, e.g. an `IconButton`), for actions used now and then that
 * should not each take a toolbar button.
 */
export function DropdownMenuButton({ items, trigger }: { items: MenuItem[]; trigger: ReactNode }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={4}
          // Long lists (a table's columns) scroll inside the menu.
          className={cn(
            CONTENT,
            "max-h-(--radix-dropdown-menu-content-available-height) overflow-y-auto",
          )}
        >
          {items.map((item, index) => (
            <div key={item.label}>
              {item.separatorBefore && index > 0 && (
                <DropdownMenu.Separator className="my-1 h-px bg-line" />
              )}
              {item.checked === undefined ? (
                <DropdownMenu.Item
                  onSelect={item.onSelect}
                  disabled={item.disabled}
                  className={itemClass(item)}
                >
                  {item.label}
                </DropdownMenu.Item>
              ) : (
                <DropdownMenu.CheckboxItem
                  checked={item.checked}
                  disabled={item.disabled}
                  onSelect={(e) => {
                    e.preventDefault();
                    item.onSelect();
                  }}
                  className={cn(itemClass(item), "gap-2 pl-1.5")}
                >
                  <span className="flex size-4 shrink-0 items-center justify-center">
                    <DropdownMenu.ItemIndicator>
                      <Check size={14} strokeWidth={2} />
                    </DropdownMenu.ItemIndicator>
                  </span>
                  <span className="truncate">{item.label}</span>
                </DropdownMenu.CheckboxItem>
              )}
            </div>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

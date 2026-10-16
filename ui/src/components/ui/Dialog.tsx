import * as RadixDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export function Dialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 440,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  return (
    <RadixDialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="animate-fade fixed inset-0 z-40 bg-black/30" />
        <RadixDialog.Content
          style={{ width }}
          className={cn(
            "animate-dialog fixed top-1/2 left-1/2 z-50 flex max-h-[85vh] max-w-[calc(100vw-32px)]",
            "-translate-x-1/2 -translate-y-1/2 flex-col rounded-xl bg-elevated shadow-dialog outline-none",
          )}
          aria-describedby={description ? undefined : undefined}
        >
          <div className="flex items-start justify-between gap-4 px-6 pt-5 pb-3">
            <div className="flex flex-col gap-1">
              <RadixDialog.Title className="font-semibold text-lg text-fg">
                {title}
              </RadixDialog.Title>
              {description && (
                <RadixDialog.Description className="text-base text-muted">
                  {description}
                </RadixDialog.Description>
              )}
            </div>
            <RadixDialog.Close
              aria-label="Close"
              className="-mr-2 inline-flex size-6 items-center justify-center rounded-md text-muted transition-colors-fast hover:bg-hover hover:text-fg"
            >
              <X size={14} strokeWidth={1.75} />
            </RadixDialog.Close>
          </div>
          {children && <div className="overflow-y-auto px-6 pb-2">{children}</div>}
          {footer && (
            <div className="flex items-center justify-end gap-2 px-6 pt-3 pb-5">{footer}</div>
          )}
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

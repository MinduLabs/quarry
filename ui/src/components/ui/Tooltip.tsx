import * as RadixTooltip from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";
import { Kbd } from "./Kbd";

export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <RadixTooltip.Provider delayDuration={500} skipDelayDuration={200}>
      {children}
    </RadixTooltip.Provider>
  );
}

export function Tooltip({
  content,
  shortcut,
  children,
}: {
  content: ReactNode;
  shortcut?: string;
  children: ReactNode;
}) {
  return (
    <RadixTooltip.Root>
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <RadixTooltip.Portal>
        <RadixTooltip.Content
          side="bottom"
          sideOffset={6}
          className="animate-pop z-50 flex items-center gap-2 rounded-md bg-elevated px-2 py-1 text-sm text-fg shadow-popover"
        >
          {content}
          {shortcut && <Kbd keys={shortcut} />}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  );
}

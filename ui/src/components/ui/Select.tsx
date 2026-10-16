import * as RadixSelect from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";

export interface Option<T extends string> {
  value: T;
  label: string;
}

export function Select<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
}: {
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
  ariaLabel?: string;
}) {
  return (
    <RadixSelect.Root value={value} onValueChange={(v) => onChange(v as T)}>
      <RadixSelect.Trigger
        aria-label={ariaLabel}
        className="flex h-7 w-full items-center justify-between rounded-md border border-line-strong bg-app px-2 text-base text-fg transition-colors-fast hover:bg-hover focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-soft"
      >
        <RadixSelect.Value />
        <ChevronDown size={14} strokeWidth={1.75} className="text-muted" />
      </RadixSelect.Trigger>
      <RadixSelect.Portal>
        <RadixSelect.Content
          position="popper"
          sideOffset={4}
          className="animate-pop z-50 min-w-(--radix-select-trigger-width) overflow-hidden rounded-lg bg-elevated p-1 shadow-popover"
        >
          <RadixSelect.Viewport>
            {options.map((option) => (
              <RadixSelect.Item
                key={option.value}
                value={option.value}
                className="relative flex h-7 cursor-default select-none items-center rounded-md pr-2 pl-7 text-base text-fg outline-none data-highlighted:bg-accent data-highlighted:text-accent-fg"
              >
                <RadixSelect.ItemIndicator className="absolute left-2">
                  <Check size={14} strokeWidth={2} />
                </RadixSelect.ItemIndicator>
                <RadixSelect.ItemText>{option.label}</RadixSelect.ItemText>
              </RadixSelect.Item>
            ))}
          </RadixSelect.Viewport>
        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  );
}

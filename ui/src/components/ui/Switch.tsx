import * as RadixSwitch from "@radix-ui/react-switch";

export function Switch({
  checked,
  onChange,
  label,
  description,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="flex flex-col">
        <span className="text-base text-fg">{label}</span>
        {description && <span className="text-sm text-muted">{description}</span>}
      </div>
      <RadixSwitch.Root
        checked={checked}
        onCheckedChange={onChange}
        aria-label={label}
        className="relative h-[18px] w-8 shrink-0 rounded-full bg-line-strong transition-colors-fast data-[state=checked]:bg-accent"
      >
        <RadixSwitch.Thumb className="block size-3.5 translate-x-0.5 rounded-full bg-white shadow-sm transition-transform duration-(--duration-fast) ease-(--ease-out) data-[state=checked]:translate-x-4" />
      </RadixSwitch.Root>
    </div>
  );
}

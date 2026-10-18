import { Keyboard, Monitor, Moon, Palette, Sun } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Dialog } from "@/components/ui/Dialog";
import { Kbd } from "@/components/ui/Kbd";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { cn } from "@/lib/cn";
import { keysOf, SHORTCUT_GROUPS, SHORTCUTS, type ShortcutId } from "@/lib/shortcuts";
import type { ThemePref } from "@/lib/theme";
import { useApp } from "@/store/app";

type Section = "appearance" | "keyboard";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

const SECTIONS: { value: Section; label: string; icon: ReactNode }[] = [
  { value: "appearance", label: "Appearance", icon: <Palette size={16} strokeWidth={1.75} /> },
  { value: "keyboard", label: "Keyboard", icon: <Keyboard size={16} strokeWidth={1.75} /> },
];

/** Fixed size: switching sections never resizes the window; each section scrolls on its own. */
export function SettingsDialog() {
  const open = useApp((s) => s.settingsOpen);
  const setOpen = useApp((s) => s.setSettingsOpen);
  const [section, setSection] = useState<Section>("appearance");
  const current = SECTIONS.find((s) => s.value === section);

  return (
    <Dialog
      open={open}
      onClose={() => setOpen(false)}
      title="Settings"
      width={760}
      height={520}
      flush
    >
      <nav
        aria-label="Settings sections"
        className="flex w-48 shrink-0 flex-col gap-0.5 border-line border-r p-2"
      >
        {SECTIONS.map((item) => (
          <button
            key={item.value}
            type="button"
            aria-current={item.value === section ? "page" : undefined}
            onClick={() => setSection(item.value)}
            className={cn(
              "flex h-8 items-center gap-2 rounded-md px-2 text-left text-base transition-colors-fast",
              "outline-none focus-visible:ring-3 focus-visible:ring-accent-soft",
              item.value === section
                ? "bg-selected text-fg"
                : "text-muted hover:bg-hover hover:text-fg",
            )}
          >
            {item.icon}
            {item.label}
          </button>
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto px-6 py-5">
        <h3 className="mb-4 font-semibold text-base text-fg">{current?.label}</h3>
        {section === "appearance" ? <AppearanceSection /> : <KeyboardSection />}
      </div>
    </Dialog>
  );
}

function AppearanceSection() {
  const theme = useApp((s) => s.theme);
  const setTheme = useApp((s) => s.setTheme);
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="flex flex-col gap-0.5">
        <span className="text-base text-fg">Theme</span>
        <span className="text-sm text-muted">
          System follows your computer's light or dark mode.
        </span>
      </div>
      <SegmentedTabs<ThemePref>
        label="Theme"
        value={theme}
        onChange={setTheme}
        className="w-64 shrink-0"
        tabs={[
          { value: "system", label: "System", icon: <Monitor {...ICON} /> },
          { value: "light", label: "Light", icon: <Sun {...ICON} /> },
          { value: "dark", label: "Dark", icon: <Moon {...ICON} /> },
        ]}
      />
    </div>
  );
}

function KeyboardSection() {
  const ids = Object.keys(SHORTCUTS) as ShortcutId[];
  return (
    <div className="flex flex-col gap-4">
      {SHORTCUT_GROUPS.map((group) => (
        <section key={group} className="flex flex-col gap-1">
          <h3 className="font-semibold text-sm text-muted">{group}</h3>
          <dl className="flex flex-col">
            {ids
              .filter((id) => SHORTCUTS[id].group === group)
              .map((id) => (
                <div
                  key={id}
                  className="flex h-8 items-center justify-between border-line border-b last:border-b-0"
                >
                  <dt className="text-base text-fg">{SHORTCUTS[id].label}</dt>
                  <dd>
                    <Kbd keys={keysOf(id)} />
                  </dd>
                </div>
              ))}
          </dl>
        </section>
      ))}
    </div>
  );
}

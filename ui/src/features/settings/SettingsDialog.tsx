import { Keyboard, Monitor, Moon, Palette, Search, Sun } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { Kbd } from "@/components/ui/Kbd";
import { SegmentedTabs } from "@/components/ui/SegmentedTabs";
import { cn } from "@/lib/cn";
import { isMac } from "@/lib/platform";
import { keyCaps, keysOf, SHORTCUT_GROUPS, SHORTCUTS, type ShortcutId } from "@/lib/shortcuts";
import type { ThemePref } from "@/lib/theme";
import { useApp } from "@/store/app";

type Section = "appearance" | "keyboard";

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/** Words each setting is found by, besides its label. A new setting adds its entry here. */
const THEME_WORDS = "appearance theme light dark system mode color colour";

/** Every name a key goes by, so "ctrl shift f", "cmd+y" or "⌘Y" all find a shortcut. */
const KEY_NAMES: Record<string, string[]> = {
  Mod: isMac ? ["cmd", "command", "⌘", "mod"] : ["ctrl", "control", "mod"],
  Ctrl: ["ctrl", "control", "⌃"],
  Alt: isMac ? ["option", "opt", "alt", "⌥"] : ["alt"],
  Shift: ["shift", "⇧"],
  Enter: ["enter", "return", "↩"],
  Delete: ["delete", "del", "backspace", "⌫"],
  Escape: ["escape", "esc", "⎋"],
  Space: ["space"],
};

/** The query as lowercase words; `+` and spaces separate keys, and symbols stand alone. */
function words(query: string): string[] {
  return query
    .toLowerCase()
    .replace(/([⌘⇧⌥⌃↩⌫⎋])/g, " $1 ")
    .split(/[\s+]+/)
    .filter(Boolean);
}

function shortcutMatches(id: ShortcutId, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const { label, group } = SHORTCUTS[id];
  if (`${label} ${group}`.toLowerCase().includes(q)) return true;
  // By keys: every typed word names one of the chord's keys.
  const keys = keysOf(id).map((k) => KEY_NAMES[k] ?? [k.toLowerCase()]);
  const typed = words(q);
  return (
    typed.length > 0 &&
    typed.every((w) => keys.some((names) => names.some((n) => n === w))) &&
    keyCaps(keysOf(id)).length >= typed.length
  );
}

const SECTIONS: { value: Section; label: string; icon: ReactNode }[] = [
  { value: "appearance", label: "Appearance", icon: <Palette size={16} strokeWidth={1.75} /> },
  { value: "keyboard", label: "Keyboard", icon: <Keyboard size={16} strokeWidth={1.75} /> },
];

/** Fixed size: switching sections never resizes the window; each section scrolls on its own. */
export function SettingsDialog() {
  const open = useApp((s) => s.settingsOpen);
  const setOpen = useApp((s) => s.setSettingsOpen);
  const [section, setSection] = useState<Section>("appearance");
  const [query, setQuery] = useState("");
  const current = SECTIONS.find((s) => s.value === section);
  const searching = query.trim() !== "";
  const themeFound =
    !searching ||
    words(query).every((w) => THEME_WORDS.split(" ").some((word) => word.startsWith(w)));
  const shortcuts = (Object.keys(SHORTCUTS) as ShortcutId[]).filter((id) =>
    shortcutMatches(id, query),
  );

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
        <div className="relative mb-2">
          <Search
            size={13}
            strokeWidth={1.75}
            className="pointer-events-none absolute top-2 left-2.5 text-faint"
          />
          <Input
            type="search"
            autoFocus
            aria-label="Search settings"
            value={query}
            placeholder="Search"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              // Escape clears the search first; a second Escape closes the dialog.
              if (e.key === "Escape" && query) {
                e.preventDefault();
                e.stopPropagation();
                setQuery("");
              }
            }}
            className="pl-7 [&::-webkit-search-cancel-button]:hidden"
          />
        </div>
        {SECTIONS.map((item) => (
          <button
            key={item.value}
            type="button"
            aria-current={!searching && item.value === section ? "page" : undefined}
            onClick={() => {
              setQuery("");
              setSection(item.value);
            }}
            className={cn(
              "flex h-8 items-center gap-2 rounded-md px-2 text-left text-base transition-colors-fast",
              "outline-none focus-visible:ring-3 focus-visible:ring-accent-soft",
              !searching && item.value === section
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
        {searching ? (
          // Results from every section at once, under their section names.
          <div className="flex flex-col gap-6">
            {themeFound && (
              <section>
                <h3 className="mb-4 font-semibold text-base text-fg">Appearance</h3>
                <AppearanceSection />
              </section>
            )}
            {shortcuts.length > 0 && (
              <section>
                <h3 className="mb-4 font-semibold text-base text-fg">Keyboard</h3>
                <KeyboardSection ids={shortcuts} />
              </section>
            )}
            {!themeFound && shortcuts.length === 0 && (
              <p className="pt-16 text-center text-base text-muted">No matching settings</p>
            )}
          </div>
        ) : (
          <>
            <h3 className="mb-4 font-semibold text-base text-fg">{current?.label}</h3>
            {section === "appearance" ? <AppearanceSection /> : <KeyboardSection ids={shortcuts} />}
          </>
        )}
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

/** Shortcuts by group; groups with none of `ids` are left out. */
function KeyboardSection({ ids }: { ids: ShortcutId[] }) {
  return (
    <div className="flex flex-col gap-4">
      {SHORTCUT_GROUPS.filter((group) => ids.some((id) => SHORTCUTS[id].group === group)).map(
        (group) => (
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
        ),
      )}
    </div>
  );
}

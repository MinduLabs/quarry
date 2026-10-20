import { isMac } from "./platform";

/**
 * Every keyboard shortcut the UI shows, in one place: tooltips, menus, the welcome screen and
 * Settings › Keyboard read from here. Keys are written once, platform-neutral: `Mod` is ⌘ on
 * macOS and Ctrl elsewhere. Keep this list in step with the handlers that bind them
 * (`App.tsx`, `SqlEditor.tsx`, `QueryView.tsx`, `ResultGrid.tsx`).
 */
export const SHORTCUTS = {
  newConnection: { label: "New connection", group: "General", keys: ["Mod", "N"] },
  newQuery: { label: "New query", group: "General", keys: ["Mod", "T"] },
  reopenTab: { label: "Reopen closed tab", group: "General", keys: ["Mod", "Shift", "T"] },
  closeTab: { label: "Close tab (not a pinned one)", group: "General", keys: ["Mod", "W"] },
  nextTab: { label: "Next tab", group: "General", keys: ["Ctrl", "Tab"] },
  previousTab: { label: "Previous tab", group: "General", keys: ["Ctrl", "Shift", "Tab"] },
  goToTab: { label: "Go to tab 1–8, or the last tab", group: "General", keys: ["Mod", "1…9"] },
  refresh: { label: "Refresh results", group: "General", keys: ["Mod", "R"] },
  toggleSidebar: { label: "Show or hide the sidebar", group: "General", keys: ["Mod", "B"] },
  settings: { label: "Settings", group: "General", keys: ["Mod", ","] },
  quickOpen: { label: "Open a table or connection", group: "General", keys: ["Mod", "P"] },
  commands: { label: "Show all commands", group: "General", keys: ["Mod", "Shift", "P"] },

  run: { label: "Run statement", group: "Editor", keys: ["Mod", "Enter"] },
  runAll: { label: "Run all", group: "Editor", keys: ["Mod", "Shift", "Enter"] },
  format: { label: "Format SQL", group: "Editor", keys: ["Mod", "Shift", "F"] },
  complete: { label: "Show completions", group: "Editor", keys: ["Ctrl", "Space"] },
  comment: { label: "Comment or uncomment", group: "Editor", keys: ["Mod", "/"] },
  find: { label: "Find in the editor", group: "Editor", keys: ["Mod", "F"] },
  // ⌘H hides the app on macOS, so the Mac uses ⌘Y (Safari's history key).
  history: { label: "Query history", group: "Editor", keys: ["Mod", isMac ? "Y" : "H"] },

  editCell: { label: "Edit cell", group: "Results", keys: ["Enter"] },
  findInResults: { label: "Find in results", group: "Results", keys: ["Mod", "F"] },
  openValue: { label: "Open the full value", group: "Results", keys: ["Shift", "Enter"] },
  selectAll: { label: "Select all cells", group: "Results", keys: ["Mod", "A"] },
  extendSelection: { label: "Extend the selection", group: "Results", keys: ["Shift", "↓"] },
  copyCell: { label: "Copy selection", group: "Results", keys: ["Mod", "C"] },
  deleteRow: { label: "Delete or keep row", group: "Results", keys: ["Delete"] },
  saveChanges: { label: "Save changes", group: "Results", keys: ["Mod", "S"] },
  undoEdit: { label: "Undo a change", group: "Results", keys: ["Mod", "Z"] },
  redoEdit: { label: "Redo a change", group: "Results", keys: ["Mod", "Shift", "Z"] },
} as const satisfies Record<string, { label: string; group: string; keys: readonly string[] }>;

export type ShortcutId = keyof typeof SHORTCUTS;

export const SHORTCUT_GROUPS = ["General", "Editor", "Results"] as const;

/** Modifiers in the order each platform prints them. */
const ORDER = isMac ? ["Ctrl", "Alt", "Shift", "Mod"] : ["Mod", "Ctrl", "Alt", "Shift"];

const MAC: Record<string, string> = {
  Mod: "⌘",
  Ctrl: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Enter: "↩",
  Delete: "⌫",
  Escape: "⎋",
  Space: "Space",
};

const OTHER: Record<string, string> = {
  Mod: "Ctrl",
  Ctrl: "Ctrl",
  Alt: "Alt",
  Shift: "Shift",
  Enter: "Enter",
  Delete: "Delete",
  Escape: "Esc",
  Space: "Space",
};

/** The key caps to draw for a chord on this platform, modifiers first. */
export function keyCaps(keys: readonly string[]): string[] {
  const rank = (key: string) => {
    const i = ORDER.indexOf(key);
    return i < 0 ? ORDER.length : i;
  };
  const names = isMac ? MAC : OTHER;
  return [...keys].sort((a, b) => rank(a) - rank(b)).map((key) => names[key] ?? key);
}

/** The chord of a shortcut, for `Kbd` and `shortcut` props. */
export function keysOf(id: ShortcutId): readonly string[] {
  return SHORTCUTS[id].keys;
}

/** Plain text for a `title` attribute, e.g. "Ctrl+S" or "⌘S". */
export function shortcutText(id: ShortcutId): string {
  return keyCaps(keysOf(id)).join(isMac ? "" : "+");
}

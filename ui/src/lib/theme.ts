/**
 * Light / dark theme. The choice is a per-machine view preference, kept in `localStorage` like the
 * sidebar state, so it applies before the engine answers and nothing flashes at startup.
 */
export type ThemePref = "system" | "light" | "dark";

/** Also read by ui/public/boot.js before the first paint; change both together. */
const KEY = "quarry.theme";
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

export function loadTheme(): ThemePref {
  try {
    const saved = localStorage.getItem(KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  } catch {
    return "system";
  }
}

export function saveTheme(pref: ThemePref) {
  try {
    localStorage.setItem(KEY, pref);
  } catch {
    // Not remembered; the theme still changes for this run.
  }
}

/** Set `data-theme` on the root element, which `tokens.css` reads. */
export function applyTheme(pref: ThemePref) {
  const dark = pref === "dark" || (pref === "system" && systemDark.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

/** Follow OS light/dark changes while the preference is System. */
export function followSystemTheme(current: () => ThemePref) {
  systemDark.addEventListener("change", () => {
    if (current() === "system") applyTheme("system");
  });
}

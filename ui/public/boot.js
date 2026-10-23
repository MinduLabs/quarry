// The theme before the first paint, read the way ui/src/lib/theme.ts reads it, so the splash and
// the app never start on the wrong background. A file, not an inline script, for the CSP.
(() => {
  let saved = null;
  try {
    saved = localStorage.getItem("quarry.theme");
  } catch {
    // No storage: follow the system.
  }
  const isDark =
    saved === "dark" ||
    (saved !== "light" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = isDark ? "dark" : "light";
})();

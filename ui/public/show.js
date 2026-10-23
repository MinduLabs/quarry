// The window opens hidden (tauri.conf.json, tauri.linux.conf.json); show it now that the splash
// above is in the page and its stylesheet has loaded (a classic script waits for it), so the first
// thing seen is the splash, never an empty white window. Outside Tauri there is nothing to show.
window.__TAURI_INTERNALS__?.invoke("plugin:window|show", { label: "main" });

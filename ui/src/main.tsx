import "./styles/base.css";

import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { hideSplash } from "./lib/splash";
import { applyTheme, followSystemTheme } from "./lib/theme";
import { useApp } from "./store/app";

// `./qd` drives the dev app through this bridge (docs/DEVTOOLS.md). Vite drops it from release builds.
if (import.meta.env.DEV) void import("./lib/devtools");

// Before the first render, so the window never shows the wrong theme.
applyTheme(useApp.getState().theme);
followSystemTheme(() => useApp.getState().theme);

/** Removes the startup splash once the first render has committed. */
function SplashDone() {
  useEffect(hideSplash, []);
  return null;
}

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
      <SplashDone />
    </StrictMode>,
  );
}

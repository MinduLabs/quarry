import "./styles/base.css";

import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { hideSplash } from "./lib/splash";
import { applyTheme, followSystemTheme } from "./lib/theme";
import { useApp } from "./store/app";

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

import "./styles/base.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyTheme, followSystemTheme } from "./lib/theme";
import { useApp } from "./store/app";

// Before the first render, so the window never shows the wrong theme.
applyTheme(useApp.getState().theme);
followSystemTheme(() => useApp.getState().theme);

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

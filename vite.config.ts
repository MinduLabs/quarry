import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// The frontend lives in ui/; Tauri loads ui/dist in release builds (src-tauri/tauri.conf.json).
export default defineConfig({
  root: "ui",
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./ui/src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // A desktop app loads from disk; one bundle is fine.
    chunkSizeWarningLimit: 1500,
    target: ["safari16", "chrome110"],
  },
});

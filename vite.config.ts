import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

/**
 * WebKitGTK (2.50 and later) stores the dev server's large responses (the SQL parser bundle and
 * its source map, megabytes each) in its disk cache, then fails to read them back on the next
 * start ("internallyFailedLoadTimerFired"). The failed load drops Vite's HMR socket and Vite
 * reloads the page, over and over. In dev nothing needs caching, so nothing is stored.
 */
function noStoreInDev(): Plugin {
  return {
    name: "quarry:no-store-in-dev",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use((_req, res, next) => {
        const setHeader = res.setHeader.bind(res);
        res.setHeader = (name, value) =>
          setHeader(name, name.toLowerCase() === "cache-control" ? "no-store" : value);
        res.setHeader("Cache-Control", "no-store");
        next();
      });
    },
  };
}

// The frontend lives in ui/; Tauri loads ui/dist in release builds (src-tauri/tauri.conf.json).
export default defineConfig({
  root: "ui",
  plugins: [react(), tailwindcss(), noStoreInDev()],
  resolve: { alias: { "@": fileURLToPath(new URL("./ui/src", import.meta.url)) } },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  // Only the SQL parser worker imports it, and Vite's scan starts from index.html, so it would be
  // found mid-session and reload the webview, which the app does not survive in dev.
  optimizeDeps: { include: ["dt-sql-parser"] },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // A desktop app loads from disk; one bundle is fine.
    chunkSizeWarningLimit: 1500,
    target: ["safari16", "chrome110"],
  },
});

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev port and its own env-driven config.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  // Tauri talks to the dev server; keep the port stable and fail loudly.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 5174 }
      : undefined,
    watch: {
      // The Rust side and generated bindings are built by cargo/tauri.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Match the webviews Tauri 2 supports.
    target: ["es2021", "chrome100", "safari15"],
    outDir: "dist",
    emptyOutDir: true,
  },
});

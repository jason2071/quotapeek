import { defineConfig } from "vite";

// Tauri expects a fixed port and no clearing of the screen so Rust logs stay visible.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Don't watch the Rust side — Tauri handles that.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    rollupOptions: {
      // Multi-page: widget (index) + settings panel.
      input: {
        index: "index.html",
        settings: "settings.html",
      },
    },
  },
});

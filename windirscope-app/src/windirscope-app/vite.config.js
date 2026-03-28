import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  plugins: [react()],

  // Prevent Vite from obscuring Rust errors
  clearScreen: false,

  // Tauri expects a fixed port
  server: {
    port: 1420,
    strictPort: true,
  },

  // Multi-page: main + results window
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        results: resolve(__dirname, "results.html"),
      },
    },
  },
});

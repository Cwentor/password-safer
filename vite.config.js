import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    host: true,
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "esnext",
    outDir: "dist",
  },
  optimizeDeps: {
    exclude: [
      '@tauri-apps/plugin-dialog',
      '@tauri-apps/plugin-notification',
      '@tauri-apps/plugin-os',
      '@tauri-apps/plugin-shell'
    ]
  }
});

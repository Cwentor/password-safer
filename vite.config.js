import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    host: true,
    port: 5173,
    strictPort: true,
    // 不监听 Rust 构建产物：target 下的 dll 可能被编译器/杀软短暂锁定，
    // watch EBUSY 会把整个 dev server 搞崩（且产物变化也不应触发热重载）
    watch: {
      ignored: ["**/src-tauri/target/**", "**/dist/**"],
    },
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
      '@tauri-apps/plugin-shell',
      '@tauri-apps/plugin-fs'
    ]
  }
});

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    // why: 1420 (Tauri scaffold default) falls inside a Windows reserved TCP
    // exclusion range (1400-1499 via Hyper-V/WSL), causing EACCES on bind.
    // 5173 (vite's own default) is outside all excluded ranges.
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1430 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});

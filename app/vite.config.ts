import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Port 1440: 1420 is hablo, 1430 is WinzAMP.
// In a plain browser, /api is proxied to tally-devserver (cargo run -p tally-devserver).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1440,
    strictPort: true,
    proxy: { "/api": "http://127.0.0.1:1441" },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: { target: "safari15", outDir: "dist" },
});

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Porta fixa: o Tauri aponta para http://localhost:1420 em desenvolvimento.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "localhost",
  },
  build: {
    target: "es2022",
    sourcemap: false,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});

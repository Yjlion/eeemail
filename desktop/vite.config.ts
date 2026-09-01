import { defineConfig } from "vite";

// Tauri serves the frontend from a fixed port in development and from bundled
// files in a release build. `clearScreen: false` keeps cargo's output visible
// when both are running.
export default defineConfig({
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});

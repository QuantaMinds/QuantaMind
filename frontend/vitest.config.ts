import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Tests only (never the dev server): allow serving the repo root so guard tests can
  // `?raw`-import Rust sources and backend/capabilities JSON — the TS↔Rust round-trip
  // guards read the REAL backend files instead of hand-maintained mirrors.
  server: { fs: { allow: [".."] } },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});

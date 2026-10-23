import { defineConfig, devices } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DATA_DIR } from "./e2e/paths";

const here = path.dirname(fileURLToPath(import.meta.url));

// End-to-end tests: the real UI (Vite) against the real backend (tally-devserver),
// exactly as the desktop app pairs them. Everything a run creates lives in
// e2e/.data, wiped at the start, so a run never sees your own books.

export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/global-setup.ts",
  // One backend, one open book at a time: tests take turns.
  workers: 1,
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  timeout: 30_000,
  expect: { timeout: 7_000 },
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: "http://localhost:1440",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    colorScheme: "light",
    viewport: { width: 1440, height: 900 },
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } } }],
  webServer: [
    {
      // Never reuse a server that's already running: it might be pointed at real books.
      command: "cargo run -q -p tally-devserver",
      cwd: path.resolve(here, ".."),
      port: 1441,
      reuseExistingServer: false,
      timeout: 600_000,
      // NO_TRASH: books the tests delete are removed outright, not sent to the real Trash.
      env: { TALLYBOOK_DIR: DATA_DIR, TALLYBOOK_NO_TRASH: "1" },
    },
    {
      command: "npx vite --port 1440 --strictPort",
      port: 1440,
      reuseExistingServer: false,
      timeout: 120_000,
    },
  ],
});

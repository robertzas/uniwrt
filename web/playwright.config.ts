import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

/*
 * The end-to-end smoke test (docs/dev/TESTING.md): the real binary with the UI embedded, serving
 * the simulated fleet (`serve --demo`, whose admin is demo / demo), driven in the installed Chrome.
 * `just e2e` builds the binary; CI passes the release build in UNIWRT_BIN.
 */
const bin = process.env.UNIWRT_BIN ?? "../target/debug/uniwrt";
const data = mkdtempSync(join(tmpdir(), "uniwrt-e2e-"));
const ci = Boolean(process.env.CI);

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  forbidOnly: ci,
  retries: ci ? 1 : 0,
  reporter: ci ? [["github"], ["list"]] : "list",
  use: {
    baseURL: "https://127.0.0.1:18443",
    // The controller's own CA signs its certificate (D-039).
    ignoreHTTPSErrors: true,
    channel: "chrome",
    trace: "retain-on-failure",
  },
  webServer: {
    command: [
      bin,
      "serve --demo --demo-seed 42",
      "--http-listen 127.0.0.1:18080 --https-listen 127.0.0.1:18443",
      `--data-dir ${data}`,
    ].join(" "),
    url: "http://127.0.0.1:18080/api/v1/health",
    timeout: 60_000,
    reuseExistingServer: false,
    stdout: "ignore",
    stderr: "pipe",
  },
});

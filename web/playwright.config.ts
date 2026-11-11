import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

/*
 * The end-to-end smoke test (docs/dev/TESTING.md): the real binary with the UI embedded, serving
 * the simulated fleet (`serve --demo`, whose admin is demo / demo) and, on a second controller, a
 * factory-default gateway without a user (`--demo-factory`), driven in the installed Chrome.
 * `just e2e` builds the binary; CI passes the release build in UNIWRT_BIN.
 */
const bin = process.env.UNIWRT_BIN ?? "../target/debug/uniwrt";
const ci = Boolean(process.env.CI);

/** A controller of its own per project: `flags` after `serve`, HTTP on `http` and HTTPS on `https`. */
function controller(flags: string, http: number, https: number) {
  const data = mkdtempSync(join(tmpdir(), "uniwrt-e2e-"));
  return {
    command: [
      bin,
      `serve ${flags}`,
      `--http-listen 127.0.0.1:${http} --https-listen 127.0.0.1:${https}`,
      `--data-dir ${data}`,
    ].join(" "),
    url: `http://127.0.0.1:${http}/api/v1/health`,
    timeout: 60_000,
    reuseExistingServer: false,
    stdout: "ignore" as const,
    stderr: "pipe" as const,
  };
}

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  forbidOnly: ci,
  retries: ci ? 1 : 0,
  reporter: ci ? [["github"], ["list"]] : "list",
  use: {
    // The controller's own CA signs its certificate (D-039).
    ignoreHTTPSErrors: true,
    channel: "chrome",
    trace: "retain-on-failure",
  },
  projects: [
    // The simulated fleet, adopted but for its last access point.
    { name: "demo", testMatch: "smoke.spec.ts", use: { baseURL: "https://127.0.0.1:18443" } },
    // A factory-default gateway and no user: the first run (adoption.md, Path E).
    { name: "first-run", testMatch: "setup.spec.ts", use: { baseURL: "https://127.0.0.1:18444" } },
  ],
  webServer: [
    controller("--demo --demo-seed 42", 18080, 18443),
    controller("--demo --demo-factory --demo-seed 42", 18081, 18444),
  ],
});

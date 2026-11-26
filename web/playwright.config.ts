import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

/*
 * The end-to-end smoke test (docs/dev/TESTING.md): the real binary with the UI embedded, serving
 * the simulated fleet (`serve --demo`, whose admin is demo / demo) and, on a second controller, a
 * factory-default gateway without a user (`--demo-factory`), and on a third the demo again for the
 * adoption port's path, driven in the installed Chrome.
 * `just e2e` builds the binary; CI passes the release build in UNIWRT_BIN.
 */
const bin = process.env.UNIWRT_BIN ?? "../target/debug/uniwrt";
const ci = Boolean(process.env.CI);
/** The screens' specs (`e2e/<name>.spec.ts`), each a project with a controller of its own. */
const screens = [
  "wifi",
  "networks",
  "vpn",
  "devices",
  "clients",
  "troubleshoot",
  "updates",
  "settings",
  "floorplans",
  "rf",
  "phone",
  "topology",
];
/** A screens' demo sized beyond the default: the topology's frame rate needs 300 entities (D-149). */
const demoFlags: Record<string, string> = {
  topology: "--demo-access-points 12 --demo-clients 320",
};
/** What a screens' project uses beyond its controller: the phone sweep runs at a phone's width. */
const devices: Record<string, object> = {
  phone: { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true },
};

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
    // The demo again, for the adoption port's bench device (adoption.md, Path C).
    { name: "port", testMatch: "port.spec.ts", use: { baseURL: "https://127.0.0.1:18445" } },
    // The screens' specs, each on a demo of its own: their saves stay out of the smoke tests' site,
    // and no two workers save to one site at once (a stale save is refused).
    ...screens.map((name, i) => ({
      name,
      testMatch: `${name}.spec.ts`,
      use: { baseURL: `https://127.0.0.1:${18446 + i}`, ...devices[name] },
    })),
  ],
  webServer: [
    controller("--demo --demo-seed 42", 18080, 18443),
    controller("--demo --demo-factory --demo-seed 42", 18081, 18444),
    controller("--demo --demo-seed 42", 18082, 18445),
    ...screens.map((name, i) =>
      controller(`--demo --demo-seed 42 ${demoFlags[name] ?? ""}`.trim(), 18083 + i, 18446 + i),
    ),
  ],
});

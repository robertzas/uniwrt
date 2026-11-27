import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

/*
 * The user guide's screenshots (D-151, M9.4c): `shots/shots.spec.ts` over `serve --demo`, at a
 * laptop's size and twice its pixels, into `UNIWRT_SHOTS` (`shots-out/` by default). CI runs it on
 * each release and publishes them with the guide on GitHub Pages (`guide/build-site.ts`);
 * `just shots` runs it locally. A second controller is the factory-default gateway of the
 * first-run wizard.
 */
const bin = process.env.UNIWRT_BIN ?? "../target/debug/uniwrt";

function controller(flags: string, http: number, https: number) {
  const data = mkdtempSync(join(tmpdir(), "uniwrt-shots-"));
  return {
    command: `${bin} serve ${flags} --http-listen 127.0.0.1:${http} --https-listen 127.0.0.1:${https} --data-dir ${data}`,
    url: `http://127.0.0.1:${http}/api/v1/health`,
    timeout: 60_000,
    reuseExistingServer: false,
    stdout: "ignore" as const,
    stderr: "pipe" as const,
  };
}

export default defineConfig({
  testDir: "shots",
  timeout: 180_000,
  expect: { timeout: 20_000 },
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: 1,
  reporter: "list",
  use: {
    ignoreHTTPSErrors: true,
    channel: "chrome",
    baseURL: "https://127.0.0.1:18643",
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    colorScheme: "light",
    actionTimeout: 30_000,
    trace: "retain-on-failure",
  },
  webServer: [
    controller("--demo --demo-seed 42", 18280, 18643),
    controller("--demo --demo-factory --demo-seed 42", 18281, 18644),
  ],
});

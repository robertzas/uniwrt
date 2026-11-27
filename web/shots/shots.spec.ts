import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { expect, type Locator, type Page, test } from "@playwright/test";

/*
 * The user guide's screenshots (D-151, M9.4c), one per `![…](…/images/<name>.png)` in
 * `docs/guide/`: the site's build (`guide/build-site.ts`) fails when a screenshot is missing or
 * shown nowhere, so adding one here goes with a line in the guide. In order, on one controller:
 * the charts fill as the demo runs, so the pages that show them come last.
 */
test.describe.configure({ mode: "serial" });

const out = process.env.UNIWRT_SHOTS ?? "shots-out";
mkdirSync(out, { recursive: true });

/** Waits for `ready`, then for the page to stop loading and settle, then takes the viewport. */
async function shoot(page: Page, name: string, ready: Locator) {
  await expect(ready.first()).toBeVisible({ timeout: 90_000 });
  await expect(page.getByText(/^Loading/)).toHaveCount(0, { timeout: 30_000 });
  await page.mouse.move(1279, 799);
  // Transitions, the charts' first draw and the topology's layout.
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(out, `${name}.png`), animations: "disabled", caret: "hide" });
}

async function signIn(page: Page) {
  await page.goto("/devices");
  await page.getByLabel("Username").fill("demo");
  await page.getByLabel("Password", { exact: true }).fill("demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("table", { name: "Devices" })).toBeVisible();
}

const region = (page: Page, name: string) => page.getByRole("region", { name, exact: true });

const heading = (page: Page, name: string, level = 1) =>
  page.getByRole("heading", { level, name, exact: true });

test("the first-run wizard", async ({ page }) => {
  await page.goto("https://127.0.0.1:18644/");
  await page.getByLabel("Password", { exact: true }).fill("correct horse battery");
  await page.getByLabel("Confirm password").fill("correct horse battery");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByRole("heading", { name: "Set up your internet router" })).toBeVisible();
  await shoot(page, "first-run", page.getByText("Factory defaults"));
});

test("the pages", async ({ page }) => {
  test.setTimeout(600_000);
  await signIn(page);
  const devices = page.getByRole("table", { name: "Devices" });
  await expect(devices.getByText("Online")).toHaveCount(3, { timeout: 30_000 });
  await shoot(page, "devices", devices);

  const pages: [name: string, path: string, ready: (page: Page) => Locator][] = [
    ["clients", "/clients", (p) => p.getByRole("table", { name: "Clients" })],
    ["wifi", "/wifi", (p) => heading(p, "Wi-Fi")],
    ["networks", "/networks", (p) => heading(p, "Networks")],
    ["internet", "/internet", (p) => heading(p, "Internet")],
    ["security", "/security", (p) => heading(p, "Security")],
    ["vpn", "/vpn", (p) => heading(p, "VPN")],
    ["alerts", "/insights/alerts", (p) => heading(p, "Alerts")],
    ["updates", "/updates", (p) => heading(p, "Controller", 2)],
    ["settings", "/settings", (p) => p.locator("main h2")],
  ];
  for (const [name, path, ready] of pages) {
    await page.goto(path);
    await shoot(page, name, ready(page));
  }

  // A client's page: the busiest client the list shows first.
  await page.goto("/clients");
  await page.getByRole("table", { name: "Clients" }).locator("tbody tr").first().click();
  await expect(page).toHaveURL(/\/clients\/./);
  await shoot(page, "client", heading(page, "Connection", 2));

  // RF: a scan fills the spectrum.
  await page.goto("/insights/rf");
  await page.getByRole("button", { name: "Scan now" }).click();
  await shoot(
    page,
    "rf",
    page.getByRole("img", { name: "Networks heard on 5 GHz, by channel and signal" }),
  );

  await page.goto("/topology");
  await shoot(page, "topology", page.locator("main canvas"));

  // The gateway's page, its charts drawn once the demo's first windows are stored.
  await page.goto("/devices");
  await devices.locator("tbody tr").first().click();
  await page.getByRole("button", { name: "Last hour" }).click();
  await expect(page.getByRole("img", { name: "CPU and memory" }).locator("canvas")).toBeVisible({
    timeout: 90_000,
  });
  await shoot(page, "device", heading(page, "Radios", 2));
  await page.getByRole("tab", { name: "Tools" }).click();
  await shoot(page, "device-tools", heading(page, "Run a test", 2));

  // The demo's logs and events, a few minutes in.
  await page.goto("/insights/logs");
  await shoot(page, "logs", heading(page, "Logs"));
  await page.goto("/insights/events");
  await shoot(page, "events", heading(page, "Events"));

  await page.goto("/insights/metrics");
  await shoot(page, "metrics", heading(page, "Metrics"));

  // The dashboard, once its throughput chart has stored windows to draw (it reads again 15 s
  // after an empty answer).
  await page.goto("/");
  await region(page, "Internet").getByRole("button", { name: "Last hour" }).click();
  await expect(page.getByText("No throughput stored yet.")).toBeHidden({ timeout: 120_000 });
  await shoot(page, "dashboard", region(page, "Network health"));
});

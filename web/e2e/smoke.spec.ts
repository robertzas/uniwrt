import { expect, type Page, test } from "@playwright/test";

/*
 * Sign in to the demo controller, watch the simulated fleet, open a device and find one with ⌘K;
 * then adopt the access point the demo leaves unadopted. In order: both use one controller.
 */
test.describe.configure({ mode: "serial" });

/** Collects the page's errors, which every test expects to stay empty. */
function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  return errors;
}

async function signIn(page: Page) {
  await page.goto("/devices");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByLabel("Username").fill("demo");
  await page.getByLabel("Password", { exact: true }).fill("demo");
  await page.getByRole("button", { name: "Sign in" }).click();
}

test("signs in and watches the simulated fleet live", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);

  // The demo world: a gateway and two access points online once the agents start; the third
  // waits to be adopted.
  const table = page.getByRole("table", { name: "Devices" });
  const rows = table.locator("tbody tr");
  await expect(rows).toHaveCount(3);
  await expect(rows.first()).toContainText("Gateway");
  await expect(table.getByText("Online")).toHaveCount(3, { timeout: 20_000 });

  await rows.nth(1).click();
  await expect(page).toHaveURL(/\/devices\/\d+$/);
  await expect(page.getByRole("heading", { level: 2, name: "Radios" })).toBeVisible();
  await expect(page.getByRole("heading", { level: 2, name: "Wi-Fi clients" })).toBeVisible();
  // The stat tile's label; the CPU chart's legend says it too once its windows are stored.
  await expect(page.getByText("CPU", { exact: true }).first()).toBeVisible();

  // ⌘K finds a device by name and opens it.
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("combobox", { name: "Command palette" });
  await palette.fill("gateway");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1 })).toContainText(/gw|gateway/i);

  // The session survives a reload: the cookie carries it.
  await page.reload();
  await expect(page.getByRole("heading", { level: 2, name: "Radios" })).toBeVisible();

  await page.getByRole("button", { name: "Account" }).click();
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("charts a device's history and opens it in the Metrics explorer", async ({ page }) => {
  // The demo's first windows are stored about 35 s after it starts.
  test.setTimeout(120_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("table", { name: "Devices" }).locator("tbody tr").first().click();
  await expect(page.getByRole("heading", { name: "Performance" })).toBeVisible();
  await page.getByRole("button", { name: "Last hour" }).click();
  // The first windows are stored about 20 s after they end; uPlot draws on a canvas.
  const cpu = page.getByRole("img", { name: "CPU and memory" });
  await expect(cpu.locator("canvas")).toBeVisible({ timeout: 60_000 });
  await expect(
    page.getByRole("img", { name: "Channel utilization" }).locator("canvas"),
  ).toBeVisible();

  await page.getByRole("link", { name: "Open in Metrics" }).click();
  await expect(page).toHaveURL(/\/insights\/metrics\?entity=node%3A\d+&range=1h$/);
  await expect(page.getByRole("heading", { name: /^gateway$/i, level: 2 })).toBeVisible();
  await expect(page.getByRole("img", { name: "CPU · Memory" }).locator("canvas")).toBeVisible();
  await page.getByRole("navigation", { name: "Series" }).getByText("Whole network").click();
  await expect(page.getByRole("heading", { name: "Whole network", level: 2 })).toBeVisible();
  expect(errors).toEqual([]);
});

test("shows the network at a glance on the dashboard", async ({ page }) => {
  // The site's first throughput window is stored about 45 s after the server starts.
  test.setTimeout(90_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("table", { name: "Devices" }).waitFor();
  await page.getByRole("navigation").getByRole("link", { name: "Dashboard" }).click();
  const health = page.getByRole("region", { name: "Network health" });
  await expect(health.getByRole("link", { name: /Internet/ })).toContainText("Online");
  await expect(health.getByRole("link", { name: /Nodes/ })).toContainText("3/3");
  // The WAN's throughput needs two topology builds for a rate, then its 15 s window closed and
  // flushed: up to about 45 s after the server starts, whatever ran before.
  await expect(page.getByRole("img", { name: "Throughput" }).locator("canvas")).toBeVisible({
    timeout: 60_000,
  });
  await expect(page.getByRole("img", { name: /^Network map: 3 devices/ })).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Top clients" }).getByRole("listitem"),
  ).not.toHaveCount(0);
  await page.getByRole("link", { name: /Open the map/ }).click();
  await expect(page).toHaveURL(/\/topology$/);
  expect(errors).toEqual([]);
});

test("adopts the discovered access point through the wizard", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);

  // The gateway's leases show it; its probe reads its host key.
  const discovered = page.getByRole("region", { name: "Discovered" });
  const row = discovered.getByRole("listitem").filter({ hasText: "ap-garden" });
  await expect(row.getByText(/^SHA256:/)).toBeVisible({ timeout: 20_000 });
  await row.getByRole("button", { name: "Adopt" }).click();

  const dialog = page.getByRole("dialog", { name: "Adopt a device" });
  await expect(dialog.getByLabel("Address")).toHaveValue(/^\d+\.\d+\.\d+\.\d+$/);
  await expect(dialog.getByLabel("Address")).toBeFocused();
  await dialog.getByLabel("Root password").fill("demo");
  await dialog.getByRole("button", { name: "Continue" }).click();
  await expect(dialog.getByTestId("fingerprint")).toHaveText(/^SHA256:/);
  await dialog.getByRole("button", { name: "It matches, adopt" }).click();

  // The steps go by live, about as fast as over a LAN.
  await expect(dialog.getByText("Back up the configuration: In progress")).toBeVisible();
  await expect(dialog.getByText("Adopted. The device is watched in observe mode.")).toBeVisible({
    timeout: 20_000,
  });
  await dialog.getByRole("button", { name: "Open the device" }).click();
  await expect(page).toHaveURL(/\/devices\/\d+$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("ap-garden");
  await expect(page.getByText("Online", { exact: true }).first()).toBeVisible({ timeout: 20_000 });

  await page.goto("/devices");
  await expect(page.getByRole("table", { name: "Devices" }).locator("tbody tr")).toHaveCount(4);
  await expect(discovered).toBeHidden();
  expect(errors).toEqual([]);
});

test("maps the network, details a device and pins it", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);
  await expect(page.getByRole("table", { name: "Devices" })).toBeVisible();
  await page.getByRole("link", { name: "Topology" }).click();
  await expect(
    page.getByRole("img", { name: /^Network map: \d devices and \d+ clients$/ }),
  ).toBeVisible({
    timeout: 20_000,
  });

  // The vertex list stands in for the canvas for keyboards and screen readers.
  await page.getByRole("button", { name: /, Gateway$/ }).focus();
  const details = page.getByRole("region", { name: "Details" });
  await expect(details.getByText("Online")).toBeVisible();
  await expect(details.getByRole("link", { name: "Open device" })).toBeVisible();

  // Dragging a device in a force view pins it on the controller, for every browser.
  await page.getByRole("radio", { name: "Clients" }).click({ force: true });
  await expect(page).toHaveURL(/view=clients/);
  await page.waitForTimeout(2_000);
  const canvas = page.getByRole("img", { name: /^Network map/ });
  const box = await canvas.boundingBox();
  if (!box) throw new Error("no canvas");
  const pinned = page.waitForResponse(
    (response) =>
      response.request().method() === "PUT" && response.url().includes("/topology/layout/"),
  );
  // Find a device under the pointer (its cursor says it moves), then drag it.
  let start: { x: number; y: number } | undefined;
  for (let row = 1; row < 10 && !start; row++) {
    for (let column = 1; column < 10 && !start; column++) {
      const at = { x: box.x + (box.width * column) / 10, y: box.y + (box.height * row) / 10 };
      await page.mouse.move(at.x, at.y);
      if ((await canvas.evaluate((element) => element.style.cursor)) === "move") start = at;
    }
  }
  if (!start) throw new Error("no device found on the canvas");
  await page.mouse.down();
  await page.mouse.move(start.x + 40, start.y + 30, { steps: 5 });
  await page.mouse.up();
  expect((await pinned).status()).toBe(204);

  // A replay asks the controller for the past.
  const slider = page.getByRole("slider", { name: "Replay the last 24 hours" });
  await slider.focus();
  await page.keyboard.press("Home");
  await expect(page.getByText("Replaying")).toBeVisible();
  await page.getByRole("button", { name: "Back to live" }).click();
  await expect(page.getByText("Replaying")).toBeHidden();
  expect(errors).toEqual([]);
});

test("lists the clients, opens one, renames it and blocks it", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);
  await expect(page.getByRole("table", { name: "Devices" })).toBeVisible();
  await page.getByRole("link", { name: "Clients" }).click();
  const table = page.getByRole("table", { name: "Clients" });
  await expect(table.locator("tbody tr").first()).toBeVisible();
  await expect(page.getByText("Experience by access point")).toBeVisible();
  await page.getByRole("button", { name: "Wi-Fi", exact: true }).click();
  await table.locator("tbody tr").first().click();
  await expect(page).toHaveURL(/\/clients\/[0-9a-f:]{17}$/);
  await expect(page.getByRole("heading", { level: 2, name: "Experience" })).toBeVisible();
  await page.getByRole("button", { name: "Kick", exact: true }).click();
  const kick = page.getByRole("dialog", { name: "Kick this client" });
  await expect(kick.getByRole("button", { name: "1 min" })).toHaveAttribute("aria-pressed", "true");
  await kick.getByRole("button", { name: "Cancel" }).click();
  await expect(kick).toBeHidden();

  await page.getByRole("tab", { name: "Timeline" }).click();
  await expect(page.getByRole("table", { name: "Where it connected" })).toBeVisible();
  await page.getByRole("tab", { name: "Signal" }).click();
  await expect(page.getByRole("img", { name: "Signal" })).toBeVisible({ timeout: 20_000 });

  await page.getByRole("button", { name: "Rename" }).click();
  const dialog = page.getByRole("dialog", { name: "Rename client" });
  await dialog.getByLabel("Name").fill("Demo phone");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Demo phone");

  await page.getByRole("button", { name: "Block", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Block Demo phone?" })
    .getByRole("button", { name: "Block" })
    .click();
  await expect(page.getByRole("button", { name: "Unblock" })).toBeVisible();
  await page.getByRole("button", { name: "Unblock" }).click();
  await expect(page.getByRole("button", { name: "Block", exact: true })).toBeVisible();
  expect(errors).toEqual([]);
});

test("searches the logs, explains an entry and shows its context", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("table", { name: "Devices" }).waitFor();
  await page.getByRole("navigation").getByRole("link", { name: "Logs" }).click();
  await expect(page).toHaveURL(/\/insights\/logs$/);
  const list = page.getByRole("list", { name: "Log entries" });
  await expect(list.getByRole("listitem").first()).toBeVisible();
  await expect(page.getByRole("img", { name: /^Log volume: [1-9]/ })).toBeVisible();

  // The simulated clients roam, and hostapd lets the old access point's entry go: explained.
  await page.getByRole("button", { name: "Explained only" }).click();
  await expect(page).toHaveURL(/annotated=1/);
  const deauth = list.getByRole("button", { name: /deauthenticated due to inactivity/ }).first();
  await expect(deauth).toBeVisible({ timeout: 30_000 });
  await deauth.click();
  await expect(page.getByRole("complementary", { name: "What it means" })).toBeVisible();
  await page.getByRole("button", { name: "Show context" }).click();
  const context = page.getByRole("dialog", { name: "Context" });
  await expect(context.locator("[aria-current=true]")).toContainText("inactivity");
  await page.keyboard.press("Escape");

  // Full-text search, with the words marked.
  await page.getByRole("button", { name: "Clear filters" }).click();
  await page.getByRole("searchbox", { name: "Search the logs" }).fill("inactivity");
  await expect(page).toHaveURL(/q=inactivity/);
  await expect(list.locator("mark").first()).toHaveText("inactivity");
  await expect(list.getByRole("listitem").filter({ hasNotText: "inactivity" })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("follows the events, filters them and opens the logs around one", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("table", { name: "Devices" }).waitFor();
  // The dashboard's card shows the notable ones, such as the adoption.
  await page.getByRole("navigation").getByRole("link", { name: "Dashboard" }).click();
  const card = page.getByRole("region", { name: "Recent events" });
  await expect(card.getByText("ap-garden adopted")).toBeVisible();
  await card.getByRole("link", { name: "All events" }).click();
  await expect(page).toHaveURL(/\/insights\/events$/);

  const list = page.getByRole("region", { name: "Events" });
  await expect(list.getByRole("button", { name: /Controller started/ })).toBeVisible();
  const adopted = list.getByRole("button", { name: /ap-garden adopted/ });
  await expect(adopted).toBeVisible();
  // The simulated clients roam between the access points.
  await page.getByRole("button", { name: "Clients", exact: true }).click();
  await expect(page).toHaveURL(/categories=client/);
  await expect(list.getByRole("button", { name: /roamed from/ }).first()).toBeVisible({
    timeout: 30_000,
  });
  await page.getByRole("button", { name: "Clear filters" }).click();
  await adopted.click();
  await list.getByRole("link", { name: "Logs around this" }).click();
  await expect(page).toHaveURL(/\/insights\/logs\?from=\d+&to=\d+&nodes=\d+$/);
  await expect(page.getByText("Fixed window")).toBeVisible();
  expect(errors).toEqual([]);
});

test("watches the alerts, edits a rule and snoozes", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("table", { name: "Devices" }).waitFor();
  // The bell opens the alerts firing (the demo may have none yet) and leads to the page.
  await page.getByRole("button", { name: /^Alerts, / }).click();
  await page.getByRole("dialog").getByRole("link", { name: "Open alerts" }).click();
  await expect(page).toHaveURL(/\/insights\/alerts$/);
  await expect(page.getByRole("region", { name: "Active alerts" })).toBeVisible();

  await page.getByRole("tab", { name: "Rules" }).click();
  const builtIn = page.getByRole("region", { name: "Built in" });
  await expect(builtIn.getByText("Device offline", { exact: true })).toBeVisible();
  await builtIn.getByRole("button", { name: "Edit: Device CPU busy" }).click();
  const editor = page.getByRole("dialog", { name: "Edit “Device CPU busy”" });
  await editor.getByLabel("Threshold").fill("85");
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor).toBeHidden();
  await expect(builtIn.getByText(/CPU above 85 %/)).toBeVisible();
  await builtIn.getByRole("button", { name: "Reset: Device CPU busy" }).click();
  await expect(builtIn.getByText(/CPU above 90 %/)).toBeVisible();

  await page.getByRole("tab", { name: "Maintenance" }).click();
  const windows = page.getByRole("region", { name: "Maintenance windows" });
  await windows.getByRole("button", { name: "Snooze alerts" }).click();
  await page.getByRole("button", { name: "Every device: For 1h" }).click();
  await expect(windows.getByText("On now")).toBeVisible();
  await windows.getByRole("button", { name: "End" }).click();
  await expect(windows.getByText("No maintenance scheduled")).toBeVisible();
  await page.getByRole("tab", { name: "History" }).click();
  await expect(page.getByRole("region", { name: "Alert history" })).toBeVisible();
  expect(errors).toEqual([]);
});

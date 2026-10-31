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
  await expect(page.getByText("CPU", { exact: true })).toBeVisible();

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

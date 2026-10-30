import { expect, test } from "@playwright/test";

/* Sign in to the demo controller, watch the simulated fleet, open a device and find one with ⌘K. */

test("signs in and watches the simulated fleet live", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });

  await page.goto("/devices");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByLabel("Username").fill("demo");
  await page.getByLabel("Password", { exact: true }).fill("demo");
  await page.getByRole("button", { name: "Sign in" }).click();

  // The demo world: a gateway and three access points, all online once the agents start.
  const table = page.getByRole("table", { name: "Devices" });
  const rows = table.locator("tbody tr");
  await expect(rows).toHaveCount(4);
  await expect(rows.first()).toContainText("Gateway");
  await expect(table.getByText("Online")).toHaveCount(4, { timeout: 20_000 });

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

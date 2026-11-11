import { expect, test } from "@playwright/test";

/*
 * The first run (features/adoption.md, Path E) on `serve --demo --demo-factory`: create the
 * admin, then the wizard finds the factory-default gateway, adopts and manages it, connects it
 * to the simulated ISP (Lumen: DHCP on VLAN 201), plans the addresses, starts a Wi-Fi network
 * and applies it all, live, through the make-before-break renumbering (D-081). Then the device
 * page shows the root password adoption set.
 */
test("sets up a factory-default router from the first run to the dashboard", async ({ page }) => {
  test.setTimeout(240_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Create the admin account" })).toBeVisible();
  await page.getByLabel("Password", { exact: true }).fill("correct horse battery");
  await page.getByLabel("Confirm password").fill("correct horse battery");
  await page.getByRole("button", { name: "Create account" }).click();

  // Router: found at OpenWrt's default address, without a root password.
  await expect(page.getByRole("heading", { name: "Set up your internet router" })).toBeVisible();
  await expect(page).toHaveURL(/\/setup$/);
  await expect(page.getByText("Factory defaults")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText("at 192.168.1.1")).toBeVisible();
  await page.getByRole("button", { name: "Use this router" }).click();
  await expect(page.getByText("OpenWrt is your gateway")).toBeVisible({ timeout: 60_000 });
  await page.getByRole("button", { name: "Continue" }).click();

  // Internet: the simulated line is Lumen's; the test on the router is optional, and passes.
  await page.getByRole("button", { name: /Lumen/ }).click();
  await page.getByRole("button", { name: "Test connection" }).click();
  await expect(page.getByRole("button", { name: "Test again" })).toBeEnabled({ timeout: 90_000 });
  await expect(page.getByText(/^The internet works/)).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();

  // Addresses: the defaults move the router from 192.168.1.1 to 10.0.0.1.
  await expect(page.getByRole("heading", { name: "Plan your addresses" })).toBeVisible();
  await expect(page.getByText(/moves to 10\.0\.0\.1/)).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();

  await page.getByLabel("Network name (SSID)").fill("Maple Street");
  await page.getByLabel("Passphrase").fill("a long passphrase");
  await page.getByLabel("Country").selectOption("US");
  await page.getByRole("button", { name: "Continue" }).click();

  // Review: saved as one revision and checked with the router before anything changes.
  await expect(page.getByText("This network is renumbered")).toBeVisible();
  await page.getByRole("button", { name: "Check with the router" }).click();
  const apply = page.getByRole("button", { name: "Apply", exact: true });
  await expect(apply).toBeVisible({ timeout: 60_000 });
  await expect(page.getByText(/moves from 192\.168\.1\.1 to 10\.0\.0\.1/)).toBeVisible();
  await apply.click();
  await expect(page.getByRole("progressbar")).toBeVisible();
  await expect(page.getByText("Your router is set up")).toBeVisible({ timeout: 180_000 });
  await expect(page.getByText(/runs your network now, at 10\.0\.0\.1/)).toBeVisible();

  // The dashboard has the gateway online, on the internet.
  await page.getByRole("button", { name: "Open the dashboard" }).click();
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
  const health = page.getByRole("region", { name: "Network health" });
  await expect(health.getByRole("link", { name: /Internet/ })).toContainText("Online", {
    timeout: 60_000,
  });

  // The root password adoption set is in the vault, for admins to see.
  await page.goto("/devices");
  await page.getByRole("table", { name: "Devices" }).locator("tbody tr").first().click();
  await page.getByRole("tab", { name: "Config" }).click();
  await page.getByRole("button", { name: "Show root password" }).click();
  await expect(page.getByRole("status", { name: "Root password" })).toHaveText(
    /^([a-z0-9]{4}-){5}[a-z0-9]{4}$/,
  );
  expect(errors).toEqual([]);
});

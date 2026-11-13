import { expect, type Page, test } from "@playwright/test";

/*
 * The adoption port end to end on the demo (features/adoption.md, Path C; D-107, D-108): the
 * gateway, managed and applied, holds a revision; the sim's bench device, plugged into its lan4
 * about 20 s after the start, shows up under Waiting for adoption; adopted as an access point, it
 * is staged, lent to the LAN, given the mesh's packages and configured, until it can be placed.
 * A controller of its own: managing the gateway and the new node would change what the smoke
 * tests see.
 */

/** Collects the page's errors, which the test expects to stay empty. */
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

test("adopts the factory device on the gateway's adoption port", async ({ page }) => {
  test.setTimeout(240_000);
  const errors = watchErrors(page);
  await signIn(page);

  // Manage the gateway, then apply: Configure applies the revision the gateway holds.
  const devices = page.getByRole("table", { name: "Devices" });
  await devices.getByRole("cell", { name: "gateway Gateway" }).click();
  await page.getByRole("tab", { name: "Config" }).click();
  await page.getByRole("button", { name: "Turn management on" }).click();
  await page
    .getByRole("dialog", { name: "Turn management on for gateway?" })
    .getByRole("button", { name: "Turn on" })
    .click();
  await expect(page.getByText("Management is on")).toBeVisible({ timeout: 30_000 });
  await page.goto("/changes");
  await page.getByRole("button", { name: /^Apply revision \d+$/ }).click();
  await expect(page.getByText(/^Revision \d+ is applied and confirmed\.$/)).toBeVisible({
    timeout: 90_000,
  });

  // The bench device: found through the gateway's login.
  await page.goto("/devices");
  const card = page.getByRole("region", { name: "Waiting for adoption" });
  const row = card.getByRole("listitem").filter({ hasText: "On the adoption port (lan4)" });
  await expect(row).toBeVisible({ timeout: 60_000 });
  await row.getByRole("button", { name: "Adopt" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Keep it plugged in until it says to unplug it.");
  await expect(dialog.getByRole("button", { name: "Access point" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await dialog.getByRole("button", { name: "It matches, adopt" }).click();
  await expect(dialog).toBeHidden();

  // Its row gives way to its steps, then to where it goes.
  const bench = card.getByRole("region", { name: "On the adoption port" });
  await expect(bench.getByText("Bring the mesh's packages")).toBeVisible();
  await expect(bench.getByText(/is adopted and set up\. Unplug it and place it/)).toBeVisible({
    timeout: 150_000,
  });
  await expect(devices.locator("tbody tr")).toHaveCount(4);
  expect(errors).toEqual([]);
});

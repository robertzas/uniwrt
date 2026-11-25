import { gunzipSync } from "node:zlib";
import { expect, type Locator, type Page } from "@playwright/test";

/** Collects the page's errors, which every test expects to stay empty. */
export function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  return errors;
}

/** Signs in as the demo's admin (demo / demo), landing on Devices. */
export async function signIn(page: Page) {
  await page.goto("/devices");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByLabel("Username").fill("demo");
  await page.getByLabel("Password", { exact: true }).fill("demo");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("table", { name: "Devices" })).toBeVisible();
}

/** A page section by its heading (`<section aria-labelledby>`, a region). */
export function region(page: Page, name: string): Locator {
  return page.getByRole("region", { name, exact: true });
}

/** Waits for a save to become the site's next revision, shown in `scope` (SavedRevision). */
export async function expectSaved(scope: Locator) {
  await expect(
    scope.getByRole("status").filter({ hasText: /^Saved as revision \d+\./ }),
  ).toBeVisible();
}

/** Saves the open dialog, waits for its revision, then closes it. */
export async function saveDialog(page: Page, save = "Save") {
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: save, exact: true }).click();
  await expectSaved(dialog);
  await dialog.getByRole("button", { name: "Close" }).last().click();
  await expect(dialog).toBeHidden();
}

/**
 * Turns management on for `device` (its name in Devices) unless it is on already, and has uniwrt
 * own `packages` too (the Config tab's checkboxes), saved. Ends on the device's Config tab. A
 * retry finds the first attempt's changes, so each step skips what is done.
 */
export async function manage(page: Page, device: string, packages: string[] = []) {
  await page.goto("/devices");
  await page
    .getByRole("table", { name: "Devices" })
    .getByRole("cell", { name: new RegExp(`^${device} `) })
    .click();
  await page.getByRole("tab", { name: "Config" }).click();
  const on = page.getByText("Management is on");
  const turnOn = page.getByRole("button", { name: "Turn management on" });
  await expect(on.or(turnOn)).toBeVisible();
  if (await turnOn.isVisible()) {
    await turnOn.click();
    await page
      .getByRole("dialog", { name: `Turn management on for ${device}?` })
      .getByRole("button", { name: "Turn on" })
      .click();
  }
  await expect(on).toBeVisible({ timeout: 30_000 });
  if (packages.length === 0) return;
  const config = page.getByRole("tabpanel", { name: "Config" });
  for (const name of packages) await config.getByRole("checkbox", { name }).check();
  const save = config.getByRole("button", { name: "Save", exact: true });
  if (await save.isEnabled()) await save.click();
  await expect(save).toBeDisabled();
}

/** The paths in a `.tar.gz` (ustar: a 512-byte header before each file's padded content). */
export function tarNames(gzipped: Buffer): string[] {
  const tar = gunzipSync(gzipped);
  const names: string[] = [];
  for (let at = 0; at + 512 <= tar.length; ) {
    const header = tar.subarray(at, at + 512);
    const name = header.subarray(0, 100).toString("utf8").replace(/\0.*$/s, "");
    if (!name) break;
    const prefix = header.subarray(345, 500).toString("utf8").replace(/\0.*$/s, "");
    names.push(prefix ? `${prefix}/${name}` : name);
    const size = Number.parseInt(header.subarray(124, 136).toString("utf8").trim() || "0", 8);
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return names;
}

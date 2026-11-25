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

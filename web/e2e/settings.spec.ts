import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { expectSaved, region, signIn, watchErrors } from "./support";

/*
 * Settings on the demo (features/ui.md, Settings): the site renamed, its revision compared with
 * its parent and exported; the LEDs' schedule; an ntfy notifier; the backups' passphrase and a
 * backup taken now; a viewer added, signing in and changing their password, then deleted. Support's bundle is covered by troubleshoot.spec.ts. A retry meets its first
 * attempt's saves, so each test's values carry the retry's number.
 */
test("renames the site, compares the revision and exports it", async ({ page }, info) => {
  const errors = watchErrors(page);
  const name = `E2E home ${info.retry}`;
  await signIn(page);
  await page.goto("/settings");
  const site = region(page, "Site");
  await expect(site.getByRole("textbox", { name: "Name" })).toHaveValue(/.+/);
  await site.getByRole("textbox", { name: "Name" }).fill(name);
  await site.getByRole("button", { name: "Save" }).click();
  await expectSaved(site);
  await expect(site.getByRole("link", { name: "Review and apply" })).toHaveAttribute(
    "href",
    "/changes",
  );

  // The latest revision, against its parent.
  await page.getByRole("tab", { name: "Revisions" }).click();
  const revisions = region(page, "Revisions");
  const latest = revisions.getByRole("listitem").first();
  await latest.getByRole("button", { name: /^Revision \d+/ }).click();
  await expect(latest).toContainText("Edit the site's settings");
  const changed = latest.getByRole("listitem").filter({ hasText: /site › name/ });
  await expect(changed).toContainText("Changed");
  await expect(changed).toContainText(name);

  // Exported without secrets: the site as JSON.
  await revisions.getByRole("button", { name: "Export" }).click();
  const dialog = page.getByRole("dialog", { name: "Export the site" });
  await expect(dialog.getByRole("radio", { name: "Without secrets" })).toBeChecked();
  const download = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "Download" }).click();
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/\.json$/);
  const text = readFileSync(await file.path(), "utf8");
  expect(() => JSON.parse(text)).not.toThrow();
  expect(text).toContain(name);
  expect(errors).toEqual([]);
});

test("schedules the LEDs off", async ({ page }, info) => {
  const errors = watchErrors(page);
  const from = `22:${String(10 + info.retry).padStart(2, "0")}`;
  await signIn(page);
  await page.goto("/settings");
  const leds = region(page, "LEDs off");
  await leds.getByRole("checkbox", { name: "Turn the LEDs off at certain times" }).check();
  await leds.getByRole("textbox", { name: "From" }).fill(from);
  await leds.getByRole("button", { name: "Saturday" }).click();
  await expect(leds.getByRole("button", { name: "Saturday" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  const schedules = region(page, "Schedules");
  await schedules.getByRole("button", { name: "Save" }).click();
  await expect(schedules.getByRole("button", { name: "Save" })).toBeHidden();
  await page.reload();
  const again = region(page, "LEDs off");
  await expect(
    again.getByRole("checkbox", { name: "Turn the LEDs off at certain times" }),
  ).toBeChecked();
  await expect(again.getByRole("textbox", { name: "From" })).toHaveValue(from);
  await expect(again.getByRole("button", { name: "Saturday" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(errors).toEqual([]);
});

test("adds an ntfy notifier", async ({ page }, info) => {
  const errors = watchErrors(page);
  const name = `e2e phone ${info.retry}`;
  await signIn(page);
  await page.goto("/settings?tab=notifications");
  const notifiers = region(page, "Notifiers");
  await notifiers.getByRole("button", { name: "Add notifier" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a notifier" });
  await expect(
    dialog.getByRole("group", { name: "Send with" }).getByRole("button", { name: "ntfy" }),
  ).toHaveAttribute("aria-pressed", "true");
  await dialog.getByRole("textbox", { name: "Name" }).fill(name);
  await dialog.getByRole("textbox", { name: "Topic" }).fill(`uniwrt-e2e-${info.retry}`);
  await dialog
    .getByRole("group", { name: "Least level" })
    .getByRole("button", { name: "Critical" })
    .click();
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();
  const row = notifiers.getByRole("listitem").filter({ hasText: name });
  await expect(row).toContainText(`ntfy.sh/uniwrt-e2e-${info.retry}`);
  await expect(row).toContainText("Critical");
  await expect(row.getByRole("checkbox", { name: `Turn off ${name}` })).toBeChecked();
  await page.reload();
  await expect(
    region(page, "Notifiers").getByRole("listitem").filter({ hasText: name }),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

test("sets the backups' passphrase and backs up now", async ({ page }) => {
  test.setTimeout(120_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/settings?tab=backups");
  const passphrase = region(page, "Passphrase");
  const set = passphrase.getByRole("button", { name: "Set passphrase" });
  const change = passphrase.getByRole("button", { name: "Change passphrase" });
  await expect(set.or(change)).toBeVisible();
  if (await set.isVisible()) {
    await passphrase.getByRole("textbox", { name: "New passphrase" }).fill("e2e backup phrase");
    await passphrase.getByRole("textbox", { name: "Confirm passphrase" }).fill("e2e backup phrase");
    await set.click();
    await expect(change).toBeVisible();
  }
  const backups = region(page, "Controller backups");
  await expect(backups.getByRole("alert")).toBeHidden();
  await backups.getByRole("button", { name: "Back up now" }).click();
  await expect(backups.getByText("No backups yet.")).toBeHidden({ timeout: 90_000 });
  await expect(backups.getByRole("button", { name: "Back up now" })).toBeEnabled();
  expect(errors).toEqual([]);
});

test("adds a viewer, who changes their password, then deletes them", async ({
  page,
  browser,
}, info) => {
  const errors = watchErrors(page);
  const name = `e2e-viewer-${info.retry}`;
  const first = "the first password";
  const second = "the viewer's own password";
  await signIn(page);
  await page.goto("/settings?tab=users");
  await page.getByRole("button", { name: "Add user" }).click();
  const add = page.getByRole("dialog", { name: "Add a user" });
  await add.getByLabel("Username").fill(name);
  await add.getByRole("radio", { name: /Viewer/ }).check();
  await add.getByLabel("Password", { exact: true }).fill(first);
  await add.getByRole("button", { name: "Add user" }).click();
  const users = page.getByRole("table", { name: "Users" });
  await expect(users.getByRole("row").filter({ hasText: name })).toContainText("Viewer");

  // The viewer signs in elsewhere, sees no users and changes their password.
  const other = await browser.newContext({
    baseURL: info.project.use.baseURL,
    ignoreHTTPSErrors: true,
  });
  const viewer = await other.newPage();
  await viewer.goto("/settings?tab=users");
  await viewer.getByLabel("Username").fill(name);
  await viewer.getByLabel("Password", { exact: true }).fill(first);
  await viewer.getByRole("button", { name: "Sign in" }).click();
  await expect(viewer.getByText("Only administrators see and manage users.")).toBeVisible();
  await viewer.getByRole("button", { name: "Account" }).click();
  await viewer.getByRole("button", { name: "Change password…" }).click();
  const change = viewer.getByRole("dialog", { name: "Change your password" });
  await change.getByLabel("Current password").fill(first);
  await change.getByLabel("New password", { exact: true }).fill(second);
  await change.getByLabel("New password again").fill(second);
  await change.getByRole("button", { name: "Change password" }).click();
  await expect(change).toBeHidden();
  await expect(viewer.getByText("Password changed")).toBeVisible();
  await other.close();

  // Deleted, after asking.
  await users.getByRole("button", { name: `Delete ${name}` }).click();
  await page
    .getByRole("dialog", { name: `Delete ${name}?` })
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(users.getByRole("row").filter({ hasText: name })).toHaveCount(0);
  expect(errors).toEqual([]);
});

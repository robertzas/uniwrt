import { expect, type Locator, type Page, test } from "@playwright/test";
import { region, signIn, watchErrors } from "./support";

/*
 * The phone-width sweep (D-148; the project `phone` runs at 390 × 844 with touch): the drawer
 * reaches every page; every route fits the screen (no page-wide sideways scroll and nothing past
 * its right edge, but a tab strip, or a chart too dense to shrink, marked
 * `data-scrolls-sideways`, which scroll aside); dialogs open as sheets along the bottom edge with
 * their actions in reach (Review's only dialog, discarding, needs an applied revision, which the
 * demo's observed nodes never have); and the topology takes a tap, with its details in a bottom
 * sheet. Nothing is saved.
 */

/** The routes visited: every page, a device's, and Settings' tabs. */
const ROUTES = [
  "/",
  "/topology",
  "/devices",
  "/devices/1",
  "/clients",
  "/wifi",
  "/networks",
  "/internet",
  "/security",
  "/vpn",
  "/insights/logs",
  "/insights/events",
  "/insights/alerts",
  "/insights/metrics",
  "/insights/rf",
  "/insights/floor-plans",
  "/updates",
  "/settings",
  "/settings?tab=notifications",
  "/settings?tab=revisions",
  "/settings?tab=backups",
  "/settings?tab=support",
  "/changes",
  "/settings/import",
  // The help center (D-151): the contents and the chapter with the widest tables.
  "/help",
  "/help/networks",
];

const WIDTH = 390;
const HEIGHT = 844;

/**
 * What reaches past the screen's right edge: the page's own sideways scroll, and elements in
 * `scope` (main, or an open dialog).
 */
function overflow(page: Page, scope = "main"): Promise<string[]> {
  return page.evaluate((scope) => {
    const width = document.documentElement.clientWidth;
    const found: string[] = [];
    const root = document.scrollingElement;
    if (root && root.scrollWidth > root.clientWidth)
      found.push(`the page scrolls to ${root.scrollWidth} px`);
    const container = document.querySelector(scope);
    for (const element of container?.querySelectorAll("*") ?? []) {
      // A tab strip, or a chart too dense to shrink, scrolls sideways on purpose.
      if (element.closest('[role="tablist"], [data-scrolls-sideways]')) continue;
      const box = element.getBoundingClientRect();
      if (box.width === 0 || box.height === 0 || box.right <= width + 1) continue;
      // Name the outermost element only: its children are past the edge with it.
      const parent = element.parentElement?.getBoundingClientRect();
      if (parent && parent.right > width + 1 && element.parentElement !== container) continue;
      const label =
        element.getAttribute("aria-label") ?? element.textContent?.trim().slice(0, 40) ?? "";
      found.push(`<${element.tagName.toLowerCase()}> ${label} (to ${Math.round(box.right)} px)`);
    }
    return found;
  }, scope);
}

/** Waits for a route's page: content in main, then the network quiet. */
async function visit(page: Page, route: string) {
  await page.goto(route);
  await expect(page.getByRole("main")).not.toBeEmpty();
  await page.waitForLoadState("networkidle");
}

/**
 * Checks that an open dialog (or a sheet: `scope` selects it) is a sheet: the screen's full width,
 * along its bottom edge once it has slid in, nothing in it past the right edge, and `action` in
 * view without scrolling.
 */
async function expectSheet(
  page: Page,
  dialog: Locator,
  action: Locator,
  scope = '[role="dialog"]',
) {
  await expect
    .poll(async () => {
      const box = await dialog.boundingBox();
      return box && [box.x, box.width, Math.round(box.y + box.height)];
    })
    .toEqual([0, WIDTH, HEIGHT]);
  expect(await overflow(page, scope)).toEqual([]);
  await expect(action).toBeInViewport({ ratio: 1 });
}

test("the drawer reaches every page", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.getByRole("button", { name: "Open navigation" }).click();
  const drawer = page.getByRole("dialog", { name: "Main" });
  const names = await drawer
    .getByRole("navigation", { name: "Main" })
    .getByRole("link")
    .allTextContents();
  expect(names.length).toBeGreaterThan(10);
  await drawer.getByRole("button", { name: "Close" }).click();
  for (const name of names) {
    await page.getByRole("button", { name: "Open navigation" }).click();
    const link = drawer
      .getByRole("navigation", { name: "Main" })
      .getByRole("link", { name, exact: true });
    const path = await link.getAttribute("href");
    await link.click();
    await expect(drawer).toBeHidden();
    await expect(page).toHaveURL((url) => url.pathname === path);
    await expect(page.getByRole("main")).not.toBeEmpty();
  }
  expect(errors).toEqual([]);
});

test("every route fits the screen", async ({ page }) => {
  test.setTimeout(120_000);
  const errors = watchErrors(page);
  await signIn(page);
  for (const route of ROUTES) {
    await visit(page, route);
    expect.soft(await overflow(page), `${route} reaches past the screen at 390 px`).toEqual([]);
  }
  expect(errors).toEqual([]);
});

test("dialogs open as sheets with their actions in reach", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);

  // Adopting the discovered access point: the wizard's first step.
  const discovered = region(page, "Discovered");
  const row = discovered.getByRole("listitem").filter({ hasText: "ap-garden" });
  await row.getByRole("button", { name: "Adopt" }).click({ timeout: 20_000 });
  const adopt = page.getByRole("dialog", { name: "Adopt a device" });
  await expectSheet(page, adopt, adopt.getByRole("button", { name: "Continue" }));
  await adopt.getByRole("button", { name: "Close" }).click();
  await expect(adopt).toBeHidden();

  // A WLAN editor, taller than the screen: its body scrolls under the title and the actions.
  await visit(page, "/wifi");
  await region(page, "Wi-Fi").getByRole("button", { name: "New WLAN" }).click();
  const wlan = page.getByRole("dialog", { name: "New WLAN" });
  const save = wlan.getByRole("button", { name: "Save", exact: true });
  await expectSheet(page, wlan, save);
  expect(await wlan.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  await wlan.evaluate((element) => element.scrollTo(0, element.scrollHeight / 2));
  await expect(save).toBeInViewport({ ratio: 1 });
  await expect(wlan.getByRole("heading", { name: "New WLAN" })).toBeInViewport({ ratio: 1 });
  await wlan.getByRole("button", { name: "Close" }).first().click();
  await expect(wlan).toBeHidden();

  // A notifier's editor, whose fields scroll in a body of their own above its actions.
  await visit(page, "/settings?tab=notifications");
  await region(page, "Notifiers").getByRole("button", { name: "Add notifier" }).click();
  const notifier = page.getByRole("dialog", { name: "Add a notifier" });
  await expectSheet(page, notifier, notifier.getByRole("button", { name: "Save", exact: true }));
  await notifier.getByRole("button", { name: "Cancel" }).click();
  await expect(notifier).toBeHidden();
  expect(errors).toEqual([]);
});

test("the topology takes a tap and shows the details in a bottom sheet", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await visit(page, "/topology");
  const canvas = page.getByRole("img", { name: /^Network map/ });
  const box = await canvas.boundingBox();
  if (!box) throw new Error("no canvas");
  // Find something on the map with the mouse, which the cursor shows, then tap it.
  let found: { x: number; y: number } | undefined;
  for (let row = 1; row < 24 && !found; row++) {
    for (let column = 1; column < 12 && !found; column++) {
      const at = { x: box.x + (box.width * column) / 12, y: box.y + (box.height * row) / 24 };
      await page.mouse.move(at.x, at.y);
      if ((await canvas.evaluate((element) => element.style.cursor)) === "pointer") found = at;
    }
  }
  if (!found) throw new Error("nothing found on the canvas");
  await page.touchscreen.tap(found.x, found.y);

  const sheet = region(page, "Details");
  await expectSheet(
    page,
    sheet,
    sheet.getByRole("button", { name: "Close" }),
    'section[aria-label="Details"]',
  );
  // The grip dismisses it with a drag down.
  const grip = sheet.locator('[aria-hidden="true"].touch-none');
  const handle = await grip.boundingBox();
  if (!handle) throw new Error("no grip");
  const x = handle.x + handle.width / 2;
  await page.mouse.move(x, handle.y + 5);
  await page.mouse.down();
  await page.mouse.move(x, handle.y + 160, { steps: 6 });
  await page.mouse.up();
  await expect(sheet).toBeHidden();
  expect(errors).toEqual([]);
});

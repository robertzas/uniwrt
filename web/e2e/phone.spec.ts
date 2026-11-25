import { expect, type Page, test } from "@playwright/test";
import { signIn, watchErrors } from "./support";

/*
 * The phone-width sweep (D-148; the project `phone` runs at 390 × 844 with touch): the drawer
 * reaches every page, and every route fits the screen: no page-wide sideways scroll and nothing
 * past its right edge (but a tab strip, or a chart too dense to shrink, marked
 * `data-scrolls-sideways`, which scroll aside). The pages that don't fit yet are listed in
 * NEEDS_B with what reaches past the edge: M9.1b makes them fit and takes each off the list, and
 * this test fails for a page on the list that fits, so the list shrinks to empty.
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
];

/** The routes that don't fit at 390 px yet, and what reaches past the edge (M9.1b). */
const NEEDS_B: ReadonlyMap<string, string> = new Map<string, string>([]);

/** What reaches past the screen's right edge: the page's own sideways scroll, and elements in main. */
function overflow(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const width = document.documentElement.clientWidth;
    const found: string[] = [];
    const root = document.scrollingElement;
    if (root && root.scrollWidth > root.clientWidth)
      found.push(`the page scrolls to ${root.scrollWidth} px`);
    for (const element of document.querySelectorAll("main *")) {
      // A tab strip, or a chart too dense to shrink, scrolls sideways on purpose.
      if (element.closest('[role="tablist"], [data-scrolls-sideways]')) continue;
      const box = element.getBoundingClientRect();
      if (box.width === 0 || box.height === 0 || box.right <= width + 1) continue;
      // Name the outermost element only: its children are past the edge with it.
      const parent = element.parentElement?.getBoundingClientRect();
      if (
        parent &&
        parent.right > width + 1 &&
        element.parentElement !== document.querySelector("main")
      )
        continue;
      const label =
        element.getAttribute("aria-label") ?? element.textContent?.trim().slice(0, 40) ?? "";
      found.push(`<${element.tagName.toLowerCase()}> ${label} (to ${Math.round(box.right)} px)`);
    }
    return found;
  });
}

/** Waits for a route's page: content in main, then the network quiet. */
async function visit(page: Page, route: string) {
  await page.goto(route);
  await expect(page.getByRole("main")).not.toBeEmpty();
  await page.waitForLoadState("networkidle");
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

test("every route fits the screen, but for those listed for M9.1b", async ({ page }) => {
  test.setTimeout(120_000);
  const errors = watchErrors(page);
  await signIn(page);
  for (const route of ROUTES) {
    await visit(page, route);
    const known = NEEDS_B.get(route);
    if (known) {
      // Its content may still be arriving: wait for it to reach past the edge.
      await expect.soft
        .poll(() => overflow(page), {
          message: `${route} fits at 390 px now: take it off NEEDS_B (${known})`,
        })
        .not.toEqual([]);
    } else {
      expect.soft(await overflow(page), `${route} reaches past the screen at 390 px`).toEqual([]);
    }
  }
  expect(errors).toEqual([]);
});

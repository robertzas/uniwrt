import { expect, type Page, test } from "@playwright/test";
import { png } from "./files";
import { region, signIn, watchErrors } from "./support";

/*
 * Floor plans on the demo (features/wifi.md, Floor plans and coverage; D-118): a floor added with
 * its plan, its scale set from two points, the devices placed, the 5 GHz coverage drawn, and all of
 * it there again after a reload.
 */

/** Clicks the plan at a share of its width and height. */
async function clickPlan(page: Page, x: number, y: number) {
  const box = await page.getByRole("img", { name: "Plan of Ground" }).boundingBox();
  if (!box) throw new Error("the plan isn't shown");
  await page.mouse.click(box.x + box.width * x, box.y + box.height * y);
}

test("adds a floor, scales it, places the devices and shows coverage", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/insights/floor-plans");
  const ground = region(page, "Ground");
  await expect(page.getByRole("heading", { name: "No floor plans yet" }).or(ground)).toBeVisible();

  // A retry finds the floor its first attempt added.
  if (!(await ground.isVisible())) {
    await page.getByRole("button", { name: "Add floor" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Add floor" });
    await dialog.getByRole("textbox", { name: "Name" }).fill("Ground");
    await dialog
      .locator('input[type="file"]')
      .setInputFiles({ name: "ground.png", mimeType: "image/png", buffer: png(800, 500) });
    await dialog.getByRole("button", { name: "Add floor" }).click();
    await expect(dialog).toBeHidden();
  }
  await expect(ground.getByRole("img", { name: "Plan of Ground" })).toBeVisible();

  // The scale: two points 60 % of the plan apart are 12 m.
  await ground.getByRole("button", { name: "Set scale" }).click();
  await expect(ground.getByRole("status")).toContainText("Click one end");
  await clickPlan(page, 0.2, 0.5);
  await clickPlan(page, 0.8, 0.5);
  const scale = ground.getByRole("form", { name: "Set scale" });
  await scale.getByRole("spinbutton", { name: "Its real length" }).fill("12");
  await scale.getByRole("button", { name: "Save scale" }).click();
  await expect(scale).toBeHidden();
  await expect(ground).not.toContainText("No scale yet");

  // Each device where it hangs.
  const unplaced = region(page, "Not placed");
  for (const [device, x, y] of [
    ["gateway", 0.2, 0.5],
    ["ap-living", 0.7, 0.3],
    ["ap-office", 0.7, 0.75],
  ] as const) {
    const place = unplaced.getByRole("button", { name: `Place ${device}` });
    if (!(await place.isVisible())) continue;
    await place.click();
    await expect(ground.getByRole("status")).toContainText(`Click the plan where ${device} is`);
    await clickPlan(page, x, y);
    await expect(place).toBeHidden();
  }
  await expect(unplaced).toBeHidden();

  // Coverage on 5 GHz, from the devices' placement and how they hear each other.
  const coverage = region(page, "Coverage");
  await coverage.getByRole("button", { name: "5 GHz" }).click();
  await expect(coverage.getByRole("button", { name: "5 GHz" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  // The demo's devices hear each other, so the model is fitted to them.
  await expect(coverage).toContainText(/Fitted to \d+ measurements between placed devices/);
  await expect(coverage).toContainText("Good, −67 dBm");

  await page.reload();
  const again = region(page, "Ground");
  await expect(again.getByRole("img", { name: "Plan of Ground" })).toBeVisible();
  await expect(again).not.toContainText("No scale yet");
  await expect(region(page, "Devices")).toBeVisible();
  await expect(region(page, "Not placed")).toBeHidden();
  expect(errors).toEqual([]);
});

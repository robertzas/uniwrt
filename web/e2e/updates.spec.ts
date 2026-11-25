import { expect, test } from "@playwright/test";
import { sysupgrade } from "./files";
import { region, signIn, watchErrors } from "./support";

/*
 * The Updates page on the demo (features/lifecycle.md): an image of one's own uploaded and offered
 * to the boards it fits, its upgrade's plan, the rollout dialog, a device's image source, and the
 * offline packages refreshed. The demo's nodes are observed, so nothing is installed.
 */
const IMAGE = "openwrt-25.12.7-mediatek-filogic-glinet_gl-mt3000-squashfs-sysupgrade.bin";

test("offers an uploaded image to the boards it fits", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/updates");
  const firmware = region(page, "Firmware");
  await expect(
    firmware.getByRole("row", { name: /^gateway GL\.iNet GL-MT6000 25\.12\.6 / }),
  ).toBeVisible();

  // An image for the access points' board: the demo runs 25.12.6, so 25.12.7 is an upgrade.
  const uploads = region(page, "Uploaded images");
  const image = uploads.getByRole("listitem", { name: IMAGE });
  await expect(uploads.getByText("No images uploaded.").or(image)).toBeVisible();
  if (!(await image.isVisible())) {
    await uploads.locator('input[type="file"]').setInputFiles({
      name: IMAGE,
      mimeType: "application/octet-stream",
      buffer: sysupgrade("glinet,gl-mt3000", "25.12.7"),
    });
  }
  await expect(image).toContainText("OpenWrt 25.12.7 · r33400-0123456789 · glinet,gl-mt3000");
  await expect(image).toContainText("Fits 2 devices");
  await expect(firmware.getByRole("button", { name: "Prepare upgrade: gateway" })).toHaveCount(0);

  // Its plan: flashed as it is; an observed device can't install it.
  await firmware.getByRole("button", { name: "Prepare upgrade: ap-living" }).click();
  const plan = page.getByRole("dialog", { name: "Upgrade ap-living" });
  await expect(plan).toContainText(IMAGE);
  await expect(plan).toContainText("OpenWrt · 25.12.7");
  await expect(plan).toContainText("Flashed as it is");
  await expect(plan.getByRole("button", { name: "Install" })).toBeDisabled();
  await plan.getByRole("button", { name: "Close" }).last().click();
  await expect(plan).toBeHidden();

  // A rollout offers both access points, each waiting for management.
  await region(page, "Rolling upgrades").getByRole("button", { name: "Roll out upgrades" }).click();
  const rollout = page.getByRole("dialog", { name: "Roll out upgrades" });
  for (const device of ["ap-living", "ap-office"]) {
    await expect(rollout.getByRole("checkbox", { name: device })).toBeDisabled();
  }
  await expect(rollout.getByRole("button", { name: "Upgrade 0 devices" })).toBeDisabled();
  await rollout.getByRole("button", { name: "Cancel" }).click();
  await expect(rollout).toBeHidden();

  // Deleted, it is offered no more.
  await image.getByRole("button", { name: `Delete ${IMAGE}` }).click();
  await image.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(image).toBeHidden();
  await expect(firmware.getByRole("button", { name: "Prepare upgrade: ap-living" })).toBeHidden();
  expect(errors).toEqual([]);
});

test("sets a device's image source and refreshes the offline packages", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/updates");
  const firmware = region(page, "Firmware");
  await firmware.getByRole("button", { name: "Change: gateway" }).click();
  const source = page.getByRole("dialog", { name: "Image source of gateway" });
  await source.getByRole("radio", { name: "OpenWrt, official image" }).check();
  await source.getByRole("button", { name: "Save" }).click();
  await expect(source).toBeHidden();
  const gateway = firmware.getByRole("row", { name: /^gateway / });
  await expect(gateway).toContainText(/official image/i);
  await page.reload();
  await expect(region(page, "Firmware").getByRole("row", { name: /^gateway / })).toContainText(
    /official image/i,
  );

  const mirror = region(page, "Offline packages");
  await expect(mirror.getByRole("checkbox", { name: "Serve packages to devices" })).toBeChecked();
  const refresh = mirror.getByRole("button", { name: "Refresh now" });
  await refresh.click();
  await expect(page.getByRole("status", { name: "Refresh started" })).toBeVisible();
  await expect(refresh).toBeEnabled({ timeout: 30_000 });
  await expect(mirror.getByText(/^Refreshed .* kept$/)).toBeVisible();
  await expect(mirror).toContainText("25.12.6 · mediatek/filogic · aarch64_cortex-a53 · apk");
  expect(errors).toEqual([]);
});

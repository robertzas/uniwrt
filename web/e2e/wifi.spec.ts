import { expect, test } from "@playwright/test";
import { expectSaved, region, saveDialog, signIn, watchErrors } from "./support";

/*
 * The Wi-Fi page on the demo (features/wifi.md): a WLAN made from the guest template, the demo's
 * WLAN edited (fast roaming off), its join code, the site's steering profile and the mesh card.
 * Saves become revisions; the demo's nodes are observed, so nothing is applied.
 */
test("adds a guest WLAN, edits roaming, shows the join code and saves steering", async ({
  page,
}) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/wifi");
  const wlans = region(page, "Wi-Fi");
  await expect(wlans.getByRole("listitem", { name: "uniwrt-demo" })).toBeVisible();

  // A guest WLAN: the template picks open, encrypted where possible, with isolated clients.
  await wlans.getByRole("button", { name: "New WLAN" }).click();
  const dialog = page.getByRole("dialog", { name: "New WLAN" });
  await dialog.getByRole("button", { name: "Guest" }).click();
  await expect(dialog.getByLabel("Security mode")).toHaveValue(/.+/);
  await dialog.getByLabel("Name (SSID)").fill("e2e-guests");
  await saveDialog(page);
  const guests = wlans.getByRole("listitem", { name: "e2e-guests" });
  await expect(guests).toContainText("Open, encrypted where possible");
  await expect(guests).toContainText("Clients isolated");

  // Fast roaming off on the WPA2/WPA3 WLAN takes its warning away.
  const demo = wlans.getByRole("listitem", { name: "uniwrt-demo" });
  await expect(demo).toContainText("fast roaming (802.11r)");
  await demo.getByRole("button", { name: "Edit uniwrt-demo" }).click();
  const roaming = page.getByRole("dialog").getByRole("region", { name: "Roaming" });
  await roaming.getByRole("button", { name: "Off", exact: true }).click();
  await saveDialog(page);
  await expect(demo).not.toContainText("fast roaming (802.11r)");

  // The join code, with the passphrase beside it.
  await demo.getByRole("button", { name: "QR code uniwrt-demo" }).click();
  const join = page.getByRole("dialog", { name: "Join uniwrt-demo" });
  await expect(join.getByRole("img", { name: "Join uniwrt-demo" })).toBeVisible();
  await expect(join).toContainText("uniwrt-demo-wifi");
  await join.getByRole("button", { name: "Close" }).last().click();
  await expect(join).toBeHidden();

  // The site's steering profile.
  const steering = region(page, "Steering");
  await steering.getByRole("radio", { name: "Balanced" }).check();
  await steering.getByRole("button", { name: "Save" }).click();
  await expectSaved(steering);
  await page.reload();
  await expect(region(page, "Steering").getByRole("radio", { name: "Balanced" })).toBeChecked();
  expect(errors).toEqual([]);
});

test("moves the mesh to another band", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/wifi");
  const mesh = region(page, "Mesh backhaul");
  await expect(mesh.getByLabel("Mesh ID")).toHaveValue("uniwrt-demo-mesh");
  await mesh.getByRole("button", { name: "Auto", exact: true }).click();
  await mesh.getByRole("button", { name: "Save" }).click();
  await expectSaved(mesh);
  await page.reload();
  await expect(
    region(page, "Mesh backhaul").getByRole("button", { name: "Auto", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(errors).toEqual([]);
});

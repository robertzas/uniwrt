import { expect, test } from "@playwright/test";
import { manage, region, signIn, watchErrors } from "./support";

/*
 * A client's doctor and actions on the demo (features/wifi.md, Client actions; observability.md,
 * Troubleshooting assistants; D-084, D-144): the iPhone's findings, signal and timeline (the
 * demo's history holds an access point dropping it) over a range; then the thermostat kicked
 * from the access point it is on, once that access point's Wi-Fi is managed. The kick keeps it
 * off for no time, so a retry finds it connected again.
 */
const IPHONE = "/clients/ce%3Afa%3A95%3Ad4%3A77%3A3a";
const THERMOSTAT = "/clients/24%3A0a%3Ac4%3A5c%3Aac%3A58";

test("reads a client's doctor over a range", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto(IPHONE);
  await page.getByRole("tab", { name: "Doctor" }).click();
  const doctor = page.getByRole("tabpanel", { name: "Doctor" });
  await expect(doctor.getByRole("button", { name: "24 hours" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(region(page, "Issues")).toContainText(/No issues found|worst first/);
  await expect(doctor.getByRole("img", { name: "Signal" })).toBeVisible();
  await expect(region(page, "Timeline").getByRole("listitem").first()).toContainText(
    "Dropped by the access point",
  );
  await doctor.getByRole("button", { name: "7 days" }).click();
  await expect(doctor.getByRole("button", { name: "7 days" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(region(page, "Issues")).toContainText(/No issues found|worst first/);
  expect(errors).toEqual([]);
});

test("kicks a client off its managed access point", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto(THERMOSTAT);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("thermostat");
  const link = page.locator('dt:text-is("Access point") + dd a');
  await expect(link).toHaveText(/^ap-/);
  const ap = ((await link.textContent()) ?? "").trim();

  // A kick reaches only access points whose Wi-Fi uniwrt manages.
  await manage(page, ap, ["wireless"]);
  await page.goto(THERMOSTAT);
  await page.getByRole("button", { name: "Kick", exact: true }).click();
  const kick = page.getByRole("dialog", { name: "Kick this client" });
  await expect(kick.getByRole("button", { name: "1 min" })).toHaveAttribute("aria-pressed", "true");
  await kick.getByRole("button", { name: "No time" }).click();
  await kick.getByRole("button", { name: "Kick", exact: true }).click();
  await expect(kick).toBeHidden();
  await expect(page.getByRole("status", { name: `Kicked from ${ap}` })).toBeVisible();
  expect(errors).toEqual([]);
});

import { expect, test } from "@playwright/test";
import { manage, region, signIn, watchErrors } from "./support";

/*
 * A client's actions and doctor on the demo (features/wifi.md, Client actions; observability.md,
 * Troubleshooting assistants; D-084, D-144): the thermostat kicked from the access point it is on,
 * once that access point's Wi-Fi is managed, then its doctor over a range.
 */
const THERMOSTAT = "/clients/24%3A0a%3Ac4%3A5c%3Aac%3A58";

test("kicks a client off its managed access point and reads its doctor", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto(THERMOSTAT);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("thermostat");
  const ap = ((await page.locator('dt:text-is("Access point") + dd a').textContent()) ?? "").trim();
  expect(ap).toMatch(/^ap-/);

  // A kick reaches only access points whose Wi-Fi uniwrt manages.
  await manage(page, ap, ["wireless"]);
  await page.goto(THERMOSTAT);
  await page.getByRole("button", { name: "Kick", exact: true }).click();
  const kick = page.getByRole("dialog", { name: "Kick this client" });
  await kick.getByRole("button", { name: "10 min" }).click();
  await kick.getByRole("button", { name: "Kick", exact: true }).click();
  await expect(kick).toBeHidden();
  await expect(page.getByRole("status", { name: `Kicked from ${ap}` })).toContainText(
    "It's kept off for 10 min.",
  );

  // The doctor: its findings, signal and timeline over the range picked.
  await page.getByRole("tab", { name: "Doctor" }).click();
  const doctor = page.getByRole("tabpanel", { name: "Doctor" });
  await doctor.getByRole("button", { name: "7 days" }).click();
  await expect(doctor.getByRole("button", { name: "7 days" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(region(page, "Issues")).toContainText(/No issues found|worst first/);
  await expect(doctor.getByRole("img", { name: "Signal" })).toBeVisible();
  await expect(region(page, "Timeline").getByRole("listitem").first()).toBeVisible({
    timeout: 20_000,
  });
  expect(errors).toEqual([]);
});

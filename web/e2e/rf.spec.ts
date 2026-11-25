import { expect, test } from "@playwright/test";
import { region, signIn, watchErrors } from "./support";

/*
 * The RF page on the demo (features/wifi.md, RF): a scan of every radio fills the spectrum, the
 * radios' neighbors and the networks around; the planner's preview keeps the observed radios as
 * they run; the scan interval and the nightly start are saved as they change.
 */
test("scans the radios and previews a channel plan", async ({ page }) => {
  test.setTimeout(60_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/insights/rf");
  await page.getByRole("button", { name: "Scan now" }).click();
  const spectrum = region(page, "Spectrum");
  const chart = spectrum.getByRole("img", {
    name: "Networks heard on 5 GHz, by channel and signal",
  });
  await expect(chart).toBeVisible({ timeout: 45_000 });
  await expect(chart.getByRole("button", { name: /^HOME-4F21 · 44 · / })).toBeVisible();

  // The gateway's radio lists who shares its channel; a busy radio waits its turn (2 min).
  const radios = region(page, "Your radios");
  const radio = (device: string) =>
    radios.getByRole("listitem").filter({ has: page.getByRole("link", { name: device }) });
  await expect(radio("gateway")).toContainText(/Scanned (just now|\d+\w ago)/, { timeout: 30_000 });
  await expect(radio("gateway")).toContainText("Mesh uniwrt-demo-mesh · same channel");
  for (const device of ["ap-living", "ap-office"]) {
    await expect(radio(device)).toContainText(/Queued|Scanned/);
  }
  const networks = region(page, "Networks around");
  await networks.getByRole("button", { name: "Neighbors" }).click();
  const heard = networks.getByRole("table", { name: "Networks heard" });
  await expect(heard.getByRole("row", { name: /^HOME-4F21 / })).toBeVisible();

  // The other band has a chart of its own.
  await page.getByRole("group", { name: "Band" }).getByRole("button", { name: "2.4 GHz" }).click();
  await expect(
    spectrum.getByRole("img", { name: "Networks heard on 2.4 GHz, by channel and signal" }),
  ).toBeVisible();
  await page.getByRole("group", { name: "Band" }).getByRole("button", { name: "5 GHz" }).click();

  // The plan's preview: observed radios are kept as they run.
  const plan = region(page, "Channel plan");
  await plan.getByRole("button", { name: "Preview plan" }).click();
  const planned = plan.getByRole("table", { name: "Planned settings" });
  for (const device of ["gateway", "ap-living", "ap-office"]) {
    const row = planned.getByRole("row", { name: new RegExp(`^${device} radio1 `) });
    await expect(row).toContainText("Unchanged");
    await expect(row).toContainText("Observe mode: kept as it runs");
  }
  await expect(plan.getByRole("button", { name: "Plan again" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("saves the scan interval and the nightly start", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/insights/rf");
  await page.getByRole("combobox", { name: "Interval" }).selectOption({ label: "Every 8 h" });
  await expect(page.getByRole("status").filter({ hasText: /saved/i }).first()).toBeVisible();
  const plan = region(page, "Channel plan");
  await plan.getByRole("combobox", { name: "Starts at" }).selectOption("02:00");
  await plan
    .getByRole("group", { name: "DFS channels" })
    .getByRole("button", { name: "No weather radar" })
    .click();
  await expect(
    plan
      .getByRole("group", { name: "DFS channels" })
      .getByRole("button", { name: "No weather radar" }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.reload();
  await expect(page.getByRole("combobox", { name: "Interval" })).toHaveValue("8");
  const reloaded = region(page, "Channel plan");
  await expect(reloaded.getByRole("combobox", { name: "Starts at" })).toHaveValue(String(2 * 60));
  await expect(
    reloaded
      .getByRole("group", { name: "DFS channels" })
      .getByRole("button", { name: "No weather radar" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(errors).toEqual([]);
});

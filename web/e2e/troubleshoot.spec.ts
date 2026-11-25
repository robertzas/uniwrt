import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { region, signIn, tarNames, watchErrors } from "./support";

/*
 * The WAN monitor and the support bundle on the demo (features/observability.md, Troubleshooting
 * assistants; D-144): the latest round, a target's chart over a range, a target added in the
 * editor and probed; then the bundle downloaded with the controller's and every device's files.
 */
test("follows the connection's quality and adds a probe target", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/internet");
  const monitor = region(page, "Connection quality");
  await expect(monitor).toContainText(/Internet up/);
  const round = monitor.getByRole("table", { name: "Latest round" });
  await expect(round.getByRole("row", { name: /^ISP gateway Ping / })).toBeVisible();

  // A row charts its target; the range reaches back further.
  await round.getByRole("row", { name: /^Google Ping 8\.8\.8\.8 / }).click();
  await expect(monitor.getByRole("combobox", { name: "Target" })).toHaveValue(/google/i);
  await monitor.getByRole("button", { name: "Last 7 days" }).click();
  await expect(monitor.getByRole("button", { name: "Last 7 days" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(monitor.getByRole("img", { name: "Latency" })).toBeVisible();
  await expect(region(page, "Outages")).toBeVisible();

  // A ping target of its own joins the next round.
  await monitor.getByRole("button", { name: "Targets" }).click();
  const editor = page.getByRole("dialog", { name: "Probe targets" });
  await editor.getByRole("button", { name: "Add a target" }).click();
  const added = editor.getByRole("listitem").last();
  await added.getByRole("textbox", { name: "Name" }).fill("e2e host");
  await added.getByRole("textbox", { name: "Host" }).fill("192.0.2.9");
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect(page.getByRole("status", { name: "Targets saved: a round runs now" })).toBeVisible();
  await expect(round.getByRole("row", { name: /^e2e host Ping 192\.0\.2\.9 / })).toBeVisible({
    timeout: 45_000,
  });
  await page.reload();
  await expect(
    region(page, "Connection quality")
      .getByRole("table", { name: "Latest round" })
      .getByRole("row", { name: /^e2e host Ping / }),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

test("downloads the support bundle", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/settings?tab=support");
  const support = region(page, "Support bundle");
  const download = page.waitForEvent("download");
  await support.getByRole("button", { name: "Download the bundle" }).click();
  const bundle = await download;
  const name = bundle.suggestedFilename();
  expect(name).toMatch(/^uniwrt-support-\d{8}-\d{4}\.tar\.gz$/);
  const root = name.replace(/\.tar\.gz$/, "");
  const files = tarNames(readFileSync(await bundle.path()));
  for (const path of [
    "README.txt",
    "controller/site.json",
    "controller/wan-monitor.json",
    "nodes/1-gateway/uci.json",
    "nodes/1-gateway/snapshot/logread.txt",
  ]) {
    expect(files).toContain(`${root}/${path}`);
  }
  expect(errors).toEqual([]);
});

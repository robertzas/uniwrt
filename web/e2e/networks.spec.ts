import { expect, test } from "@playwright/test";
import { expectSaved, region, saveDialog, signIn, watchErrors } from "./support";

/*
 * The Networks page on the demo (features/network.md): a guest network from its purpose, the
 * layered upstream DNS (an encrypted resolver before the ISP's) and DNS filtering turned on; then,
 * with the gateway managed and the revision applied, the query statistics and the query log. In
 * order: the second applies what the first saved.
 */
test.describe.configure({ mode: "serial" });
test("adds a guest network, encrypts upstream DNS and turns filtering on", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/networks");
  const networks = region(page, "Networks");
  await expect(networks.getByRole("listitem", { name: "Default" })).toContainText("10.0.0.0/24");

  // A network from its purpose: the purpose picks the VLAN, a /24 and the policy.
  await networks.getByRole("button", { name: "New network" }).click();
  let dialog = page.getByRole("dialog", { name: "New network" });
  await dialog.getByRole("button", { name: /^Guest/ }).click();
  await dialog.getByRole("button", { name: "Continue" }).click();
  await expect(dialog.getByLabel("Name", { exact: true })).toHaveValue("Guest");
  await expect(dialog.getByLabel("Gateway address")).toHaveValue(/^10\.0\.\d+\.1\/24$/);
  await saveDialog(page);
  await expect(networks.getByRole("listitem", { name: "Guest" })).toBeVisible();

  // Upstream DNS: Quad9 over TLS first, then the ISP's servers.
  const upstream = region(page, "Upstream DNS");
  await upstream.getByRole("button", { name: "Edit" }).click();
  await expect(upstream.getByRole("button", { name: "DNS over TLS" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await upstream.getByRole("button", { name: "Quad9" }).click();
  await upstream.getByRole("button", { name: "Save" }).click();
  await expectSaved(upstream);
  const layers = upstream.getByRole("list", { name: "Layers, asked in turn" });
  await expect(layers.getByRole("listitem")).toHaveCount(2);
  await expect(layers.getByRole("listitem").first()).toContainText("Quad9");
  await expect(layers.getByRole("listitem").last()).toContainText("ISP");

  // DNS filtering on, with its default lists.
  const filtering = region(page, "DNS filtering");
  await filtering.getByRole("button", { name: "Edit" }).click();
  dialog = page.getByRole("dialog", { name: "DNS filtering" });
  await dialog.getByRole("checkbox", { name: "Filter DNS on every network" }).check();
  await expect(dialog.getByRole("checkbox", { name: "HaGeZi Normal" })).toBeChecked();
  await saveDialog(page);
  await expect(filtering).not.toContainText("Off: every name resolves unfiltered.");
  expect(errors).toEqual([]);
});

test("applies filtering on the managed gateway and shows its queries", async ({ page }) => {
  test.setTimeout(180_000);
  const errors = watchErrors(page);
  await signIn(page);

  // Manage the gateway, then apply what the first test saved.
  await page
    .getByRole("table", { name: "Devices" })
    .getByRole("cell", { name: "gateway Gateway" })
    .click();
  await page.getByRole("tab", { name: "Config" }).click();
  await page.getByRole("button", { name: "Turn management on" }).click();
  await page
    .getByRole("dialog", { name: "Turn management on for gateway?" })
    .getByRole("button", { name: "Turn on" })
    .click();
  await expect(page.getByText("Management is on")).toBeVisible({ timeout: 30_000 });
  // uniwrt owns only the packages chosen: DHCP & DNS carries the filter.
  const config = page.getByRole("tabpanel", { name: "Config" });
  await config.getByRole("checkbox", { name: "dhcp" }).check();
  await config.getByRole("button", { name: "Save", exact: true }).click();
  await expect(config.getByRole("button", { name: "Save", exact: true })).toBeDisabled();
  await page.goto("/changes");
  await page.getByRole("button", { name: /^Apply revision \d+$/ }).click();
  await expect(page.getByText(/^Revision \d+ is applied and confirmed\.$/)).toBeVisible({
    timeout: 90_000,
  });

  // The gateway logs its queries now: the statistics fill in, and the log lists them.
  await page.goto("/networks");
  const queries = page.getByRole("region", { name: "DNS queries", exact: true });
  await expect(
    queries.getByRole("region", { name: "Top domains" }).getByRole("listitem").first(),
  ).toBeVisible({
    timeout: 60_000,
  });
  await queries.getByRole("button", { name: "Query log" }).click();
  const log = page.getByRole("dialog", { name: "DNS query log" });
  await expect(
    log.getByRole("table", { name: "DNS queries" }).locator("tbody tr").first(),
  ).toBeVisible();
  expect(errors).toEqual([]);
});

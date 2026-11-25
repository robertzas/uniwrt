import { expect, test } from "@playwright/test";
import { region, saveDialog, signIn, watchErrors } from "./support";

/*
 * The VPN page on the demo (features/network.md, VPN): a WireGuard server, a phone's peer and its
 * configuration, and Tailscale set up to log in from the page.
 */
test("sets up WireGuard, adds a peer with its configuration and sets up Tailscale", async ({
  page,
}) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/vpn");

  const server = region(page, "WireGuard server");
  await server.getByRole("button", { name: "Set up WireGuard" }).click();
  const setup = page.getByRole("dialog", { name: "Set up WireGuard" });
  await setup.getByLabel("Public name or address").fill("vpn.example.com");
  await saveDialog(page);
  await expect(server).toContainText("vpn.example.com:51820");
  await expect(server.getByRole("button", { name: "Copy the public key" })).toBeVisible();

  // A phone's peer, its configuration shown once saved.
  const peers = region(page, "Peers");
  await peers.getByRole("button", { name: "Add peer" }).click();
  const add = page.getByRole("dialog", { name: "Add peer" });
  await add.getByLabel("Name", { exact: true }).fill("e2e-phone");
  await add.getByRole("button", { name: "Save and show configuration" }).click();
  const config = page.getByRole("dialog").filter({ hasText: "[Interface]" });
  await expect(config).toContainText("Endpoint = vpn.example.com:51820");
  await expect(config.getByRole("img")).toBeVisible();
  await config.getByRole("button", { name: "Close" }).last().click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(peers).toContainText("e2e-phone");

  // Tailscale, logged in from the page once applied.
  const tailscale = region(page, "Tailscale");
  await tailscale.getByRole("button", { name: "Set up Tailscale" }).click();
  const ts = page.getByRole("dialog", { name: "Set up Tailscale" });
  await expect(ts.getByRole("checkbox", { name: "Default" })).toBeChecked();
  await saveDialog(page);
  await expect(tailscale.getByRole("button", { name: "Set up Tailscale" })).toBeHidden();
  expect(errors).toEqual([]);
});

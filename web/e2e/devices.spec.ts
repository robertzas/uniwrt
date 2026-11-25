import { readFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";
import { manage, region, signIn, watchErrors } from "./support";

/*
 * A device's Tools, Terminal and UCI tabs on the demo's gateway (features/observability.md,
 * Diagnostics toolkit; D-141, D-142, D-143): while it is observed, a ping, the tables, a terminal
 * session and the read-only explorer; with management on, the optional tools installed, a capture
 * saved, a throughput test, and a UCI override applied and read back from the device. In order:
 * the second manages the gateway.
 */
test.describe.configure({ mode: "serial" });

/** Opens the gateway's page on `tab`. */
async function gatewayTab(page: Page, tab: string) {
  await page.goto("/devices");
  await page
    .getByRole("table", { name: "Devices" })
    .getByRole("cell", { name: "gateway Gateway" })
    .click();
  await page.getByRole("tab", { name: tab }).click();
}

test("pings, reads the tables, opens a terminal and the UCI explorer on the observed gateway", async ({
  page,
}) => {
  const errors = watchErrors(page);
  await signIn(page);
  await gatewayTab(page, "Tools");
  await expect(region(page, "Optional tools")).toContainText("Read-only in observe mode");

  // A run streams its output, then joins the recent runs.
  const form = region(page, "Run a test");
  await form.getByRole("textbox", { name: "Host" }).fill("openwrt.org");
  await form.getByRole("spinbutton", { name: "Requests" }).fill("3");
  await form.getByRole("button", { name: "Run" }).click();
  const run = region(page, "Ping openwrt.org");
  await expect(run.getByText("Succeeded", { exact: true })).toBeVisible({ timeout: 20_000 });
  await expect(run.getByRole("log", { name: "Output of the run" })).toContainText(
    "3 packets transmitted",
  );
  await expect(
    region(page, "Recent runs").getByRole("button", { name: /^Ping openwrt\.org Succeeded/ }),
  ).toBeVisible();

  // The tables, read from the device: a client's neighbor entry links to it.
  const tables = region(page, "Tables");
  const neighbors = tables.getByRole("table", { name: "Neighbor cache" });
  await expect(neighbors.getByRole("row", { name: /^10\.0\.0\.2 / })).toBeVisible();
  await tables.getByRole("searchbox", { name: "Filter" }).fill("10.0.0.103");
  await expect(neighbors.locator("tbody tr")).toHaveCount(1);
  await expect(neighbors.getByRole("link", { name: "thermostat" })).toBeVisible();
  await tables.getByRole("tab", { name: "Routes" }).click();
  await expect(
    tables
      .getByRole("table", { name: "Routes" })
      .getByRole("row", { name: /^default 203\.0\.113\.1 eth1\.201 / }),
  ).toBeVisible();
  await tables.getByRole("tab", { name: "Connections" }).click();
  await expect(tables).toContainText(/Tracking \d+ of at most/);
  await expect(
    tables.getByRole("table", { name: "Tracked connections" }).locator("tbody tr").first(),
  ).toBeVisible();

  // Root's shell: what it prints arrives over the terminal's socket (xterm draws on a canvas).
  const received: string[] = [];
  page.on("websocket", (socket) => {
    if (!socket.url().includes("/terminal")) return;
    socket.on("framereceived", ({ payload }) => received.push(payload.toString()));
  });
  await page.getByRole("tab", { name: "Terminal" }).click();
  const terminal = region(page, "Terminal on gateway");
  await terminal.getByRole("button", { name: "Open terminal" }).click();
  await expect(terminal.getByRole("status")).toHaveText("Connected");
  await expect.poll(() => received.join("")).toContain("root@gateway:~#");
  await terminal.getByRole("textbox", { name: "Terminal input" }).focus();
  await page.keyboard.type("uname -n\n");
  await expect.poll(() => received.join("")).toMatch(/uname -n\r\ngateway\r\n/);
  await page.keyboard.type("exit\n");
  await expect(terminal.getByRole("status")).toHaveText("The shell ended");
  await expect(terminal.getByRole("button", { name: "Reconnect" })).toBeVisible();

  // The explorer reads every package, read-only in observe mode.
  await page.getByRole("tab", { name: "UCI" }).click();
  const uci = region(page, "UCI explorer of gateway");
  await expect(uci).toContainText("This device is in observe mode");
  await uci
    .getByRole("navigation", { name: "Packages" })
    .getByRole("button", { name: /^system/ })
    .click();
  const section = uci.getByRole("article", { name: "system.@system[0]" });
  await expect(section.locator('dt:text-is("hostname") + dd')).toHaveText("gateway");
  await expect(section.getByRole("button", { name: "Edit" })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("installs tools, captures, tests throughput and applies a UCI override on the managed gateway", async ({
  page,
}) => {
  test.setTimeout(180_000);
  const errors = watchErrors(page);
  await signIn(page);
  await manage(page, "gateway");
  await page.getByRole("tab", { name: "Tools" }).click();

  // The optional tools install from the device's feeds.
  const optional = region(page, "Optional tools");
  for (const pkg of ["tcpdump-mini", "iperf3"]) {
    const item = optional.getByRole("listitem").filter({ hasText: pkg });
    const install = item.getByRole("button", { name: "Install" });
    const installed = item.getByText("Installed", { exact: true });
    await expect(install.or(installed)).toBeVisible();
    if (await install.isVisible()) await install.click();
    await expect(installed).toBeVisible({ timeout: 60_000 });
  }

  // A capture on the LAN bridge saves a .pcap when it ends.
  const capture = region(page, "Packet capture");
  await capture
    .getByRole("combobox", { name: "Interface" })
    .selectOption({ label: "br-lan (lan)" });
  await capture.getByRole("spinbutton", { name: "Seconds" }).fill("3");
  const download = page.waitForEvent("download");
  await capture.getByRole("button", { name: "Capture" }).click();
  await expect(capture.getByRole("progressbar", { name: "Capturing" })).toBeVisible();
  const pcap = await download;
  expect(pcap.suggestedFilename()).toMatch(/^gateway-br-lan-\d{8}-\d{6}\.pcap$/);
  // The pcap magic, in either precision.
  expect([0xa1b2c3d4, 0xa1b23c4d]).toContain(readFileSync(await pcap.path()).readUInt32LE(0));
  await expect(capture.getByRole("status")).toContainText(`Saved ${pcap.suggestedFilename()}`);

  // A throughput test against the controller, its rate beside the output.
  const form = region(page, "Run a test");
  await form.getByRole("button", { name: "Throughput test (iperf3)" }).click();
  await expect(form.getByRole("combobox", { name: "Far end" })).toContainText("This controller");
  await form.getByRole("spinbutton", { name: "Seconds" }).fill("3");
  await form.getByRole("button", { name: "Run" }).click();
  const run = region(page, "Throughput to the controller");
  await expect(run.getByText("Succeeded", { exact: true })).toBeVisible({ timeout: 30_000 });
  await expect(run).toContainText(/Sent [\d.]+ [MG]b\/s/);
  await expect(run.getByRole("img", { name: "Throughput each second" })).toBeVisible();

  // A UCI edit becomes the site's override of the device's own file, applied.
  await page.getByRole("tab", { name: "UCI" }).click();
  let uci = region(page, "UCI explorer of gateway");
  await uci
    .getByRole("navigation", { name: "Packages" })
    .getByRole("button", { name: /^system/ })
    .click();
  let section = uci.getByRole("article", { name: "system.@system[0]" });
  await section.getByRole("button", { name: "Edit" }).click();
  const editor = section.getByRole("form", { name: "Edit system.@system[0]" });
  await editor.getByRole("textbox", { name: "Value of log_size" }).fill("64");
  await editor.getByRole("button", { name: "Save to the site" }).click();
  await expect(
    section.getByRole("region", { name: "The site's overrides of this section" }),
  ).toContainText("log_size=64");
  await page.goto("/changes");
  await page.getByRole("button", { name: /^Apply revision \d+$/ }).click();
  await expect(page.getByText(/^Revision \d+ is applied and confirmed\.$/)).toBeVisible({
    timeout: 90_000,
  });
  await gatewayTab(page, "UCI");
  uci = region(page, "UCI explorer of gateway");
  await uci
    .getByRole("navigation", { name: "Packages" })
    .getByRole("button", { name: /^system/ })
    .click();
  section = uci.getByRole("article", { name: "system.@system[0]" });
  await expect(section.locator('dt:text-is("log_size") + dd')).toHaveText("64");
  expect(errors).toEqual([]);
});

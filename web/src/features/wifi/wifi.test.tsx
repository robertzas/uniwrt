import { memoryHistory } from "@solidjs/router";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Issue } from "../../api/gen/Issue";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { SiteSave } from "../../api/gen/SiteSave";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { client } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig;
/** A refusal the next save answers with, once. */
let refusal: { path: string; message: string } | undefined;
let fleet: NodeSummary[];
/** The validation warnings the server's views carry. */
let issues: Issue[];

beforeEach(() => {
  calls = [];
  head = 2;
  refusal = undefined;
  issues = [];
  fleet = [summary(1, "gw", { role: "gateway" }), summary(2, "attic")];
  config = siteConfig();
  config.nodes = [1, 2].map((node) => ({
    node,
    hostname: null,
    managed: ["wireless"],
    radios: [],
    ports: [],
    raw: [],
  }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET")
        return Response.json({ ...siteView(head, config), issues });
      if (url === "/api/v1/site" && method === "PUT") {
        if (refusal) {
          const issues = [{ ...refusal, severity: "error" }];
          refusal = undefined;
          return Response.json(
            { title: "Unprocessable", detail: "The document has errors", issues },
            { status: 422, headers: { "content-type": "application/problem+json" } },
          );
        }
        head += 1;
        config = (body as SiteSave).config;
        return Response.json({ ...siteView(head, config), issues });
      }
      if (url === "/api/v1/site/secrets/reveal") return Response.json({ value: "correct horse" });
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderWifi(role: "admin" | "viewer" = "admin", path = "/wifi") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, fleet);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  if (path === "/wifi") {
    await waitFor(() =>
      expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["clients"]) }),
    );
    ws.receive({
      type: "clients_snapshot",
      topic: "clients",
      clients: [
        client("02:00:00:00:00:01", { connection: connection("uniwrt-demo") }),
        client("02:00:00:00:00:02", { connection: connection("Elsewhere") }),
      ],
    });
    await screen.findByRole("heading", { name: "Wi-Fi", level: 1 });
  }
  return { history, ws };
}

function connection(ssid: string) {
  const base = client("02:00:00:00:00:09").connection;
  if (!base) throw new Error("the fixture client has a connection");
  return { ...base, ssid };
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("Wi-Fi", () => {
  it("names the devices without 802.11v while a WLAN asks for roaming hints", async () => {
    await renderWifi();
    expect(screen.queryByText(/run the basic wpad/)).toBeNull();
    cleanup();
    fleet[1] = summary(2, "attic", { basic_wpad: true });
    await renderWifi();
    expect(await screen.findByText(/This device runs the basic wpad/)).toBeVisible();
    expect(screen.getByRole("link", { name: "attic" })).toHaveAttribute(
      "href",
      "/devices/2?tab=config",
    );
    cleanup();
    config.wlans = config.wlans.map((wlan) => ({
      ...wlan,
      roaming: { ...wlan.roaming, hints: false },
    }));
    await renderWifi();
    expect(screen.queryByText(/run the basic wpad/)).toBeNull();
  });

  it("lists the WLANs with their security, bands, network and clients", async () => {
    config.wlans[0] = {
      ...config.wlans[0],
      nodes: { scope: "only", nodes: [2] },
      schedule: { spans: [{ days: ["mon", "tue", "wed", "thu", "fri"], from: 420, to: 1320 }] },
    } as SiteConfig["wlans"][0];
    await renderWifi("viewer");
    const row = await screen.findByRole("listitem", { name: "uniwrt-demo" });
    expect(within(row).getByText("WPA2/WPA3")).toBeInTheDocument();
    expect(within(row).getByText("2.4 GHz")).toBeInTheDocument();
    expect(within(row).getByText("5 GHz")).toBeInTheDocument();
    expect(within(row).getByText("Network Default")).toBeInTheDocument();
    expect(within(row).getByText("Only attic")).toBeInTheDocument();
    expect(within(row).getByText("Mon–Fri 07:00–22:00")).toBeInTheDocument();
    expect(within(row).getByText(/^(on|off) now$/)).toBeInTheDocument();
    expect(await within(row).findByText("1 client online")).toBeInTheDocument();
    // A viewer neither edits nor sees the passphrase's QR code.
    expect(within(row).queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New WLAN" })).not.toBeInTheDocument();
    expect(screen.getByText(/Regulatory domain: United States/)).toBeInTheDocument();
  });

  it("warns without a country and about devices that don't manage wireless", async () => {
    config.site.country = null;
    config.nodes = config.nodes.filter((node) => node.node === 1);
    const { history } = await renderWifi();
    expect(await screen.findByText(/doesn't manage the wireless package on attic/)).toBeVisible();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Set the country" }));
    await waitFor(() => expect(history.get()).toBe("/settings"));
  });

  it("adds a guest WLAN from its preset", async () => {
    const user = userEvent.setup();
    await renderWifi();
    await user.click(await screen.findByRole("button", { name: "New WLAN" }));
    const editor = await screen.findByRole("dialog", { name: "New WLAN" });
    await user.click(within(editor).getByRole("button", { name: "Guest" }));
    expect(within(editor).getByLabelText("Security mode")).toHaveValue("owe_transition");
    expect(within(editor).queryByLabelText("Passphrase")).not.toBeInTheDocument();
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText("A name is 1 to 32 bytes.")).toBeInTheDocument();
    await user.type(within(editor).getByLabelText("Name (SSID)"), "Visitors");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().message).toBe("Add WLAN Visitors");
    expect(put().secrets).toBeUndefined();
    expect(put().config.wlans[1]).toEqual({
      id: "visitors",
      ssid: "Visitors",
      enabled: true,
      network: "lan",
      security: "owe_transition",
      passphrase: null,
      bands: ["2g", "5g"],
      nodes: { scope: "all" },
      hidden: false,
      isolation: true,
      max_clients: null,
      schedule: null,
      roaming: { fast: null, hints: true, steer: true },
    });
  });

  it("edits a WLAN: passphrase, bands, devices and limit", async () => {
    const user = userEvent.setup();
    await renderWifi();
    const row = await screen.findByRole("listitem", { name: "uniwrt-demo" });
    await user.click(within(row).getByRole("button", { name: "Edit uniwrt-demo" }));
    const editor = await screen.findByRole("dialog", { name: "WLAN uniwrt-demo" });
    await user.type(within(editor).getByLabelText("Passphrase"), "correct horse");
    // 6 GHz needs WPA3: the form says so before the server does.
    await user.click(within(editor).getByRole("button", { name: "6 GHz" }));
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/6 GHz requires WPA3 or OWE/)).toBeInTheDocument();
    await user.selectOptions(within(editor).getByLabelText("Security mode"), "wpa3");
    await user.click(within(editor).getByRole("button", { name: "All but these" }));
    await user.click(within(editor).getByLabelText("attic"));
    await user.type(within(editor).getByLabelText("Clients per radio"), "40");
    // Fast roaming: left to the mode, WPA3 goes without; forced on, the form warns.
    expect(within(editor).getByText(/Auto: on for WPA2.*Now off\./)).toBeInTheDocument();
    const fast = within(editor).getByRole("group", { name: "Fast roaming (802.11r)" });
    await user.click(within(fast).getByRole("button", { name: "On" }));
    expect(within(editor).getByText(/Some clients fail to join WPA3/)).toBeInTheDocument();
    await user.click(within(editor).getByLabelText("Roaming hints (802.11k/v)"));
    expect(within(editor).getByText(/Takes part when the site's steering is on/)).toBeVisible();
    await user.click(within(editor).getByLabelText("Steer clients"));
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().message).toBe("Edit WLAN uniwrt-demo");
    expect(put().secrets).toEqual({ "wlans/home/passphrase": "correct horse" });
    expect(put().config.wlans[0]).toMatchObject({
      id: "home",
      security: "wpa3",
      passphrase: "config/00000000000000000000000000000001",
      bands: ["2g", "5g", "6g"],
      nodes: { scope: "except", nodes: [2] },
      max_clients: 40,
      schedule: null,
      roaming: { fast: true, hints: false, steer: false },
    });
  });

  it("sets the site's steering profile, with what each does and where it falls short", async () => {
    const user = userEvent.setup();
    config.wlans = config.wlans.map((wlan) => ({
      ...wlan,
      roaming: { ...wlan.roaming, steer: false },
    }));
    fleet[1] = summary(2, "attic", { basic_wpad: true });
    const { ws } = await renderWifi();
    const card = await screen.findByRole("region", { name: "Steering" });
    const profiles = within(card).getByRole("group", { name: "Steering" });
    expect(within(profiles).getByLabelText("Off")).toBeChecked();
    expect(
      within(card).getByText(/-75 dBm is moved to an access point 8 dB stronger/),
    ).toBeVisible();
    // What these releases' hostapd does to usteer's requests is said once, under the profiles.
    expect(within(card).getByText(/hostapd refuses usteer's requests to move/)).toBeVisible();
    // Steering off: the page's note names the basic wpad, the card doesn't.
    expect(screen.getByText(/This device runs the basic wpad, so its/)).toBeVisible();
    expect(within(card).queryByRole("link", { name: "attic" })).toBeNull();
    await user.click(within(profiles).getByLabelText("Balanced"));
    // No WLAN steers: the choice says so before it is saved.
    expect(within(card).getByText(/No WLAN takes part/)).toBeVisible();
    await user.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(within(profiles).getByLabelText("Off")).toBeChecked();
    expect(within(card).queryByText(/No WLAN takes part/)).toBeNull();
    await user.click(within(profiles).getByLabelText("Balanced"));
    issues = [{ path: "steering", severity: "warning", message: "No WLAN takes part in steering" }];
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().message).toBe("Steering: Balanced");
    expect(put().config.steering).toEqual({ profile: "balanced" });
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    // The saved document's warning, and the devices where steering can only disconnect.
    expect(await within(card).findByText("No WLAN takes part in steering")).toBeVisible();
    expect(within(card).getByText(/steering can only disconnect clients there/)).toBeVisible();
    expect(within(card).getByRole("link", { name: "attic" })).toHaveAttribute(
      "href",
      "/devices/2?tab=config",
    );
    expect(screen.queryByText(/This device runs the basic wpad, so its/)).toBeNull();
  });

  it("shows the steering profile read-only to viewers", async () => {
    config.steering = { profile: "conservative" };
    await renderWifi("viewer");
    const card = await screen.findByRole("region", { name: "Steering" });
    expect(within(card).getByLabelText("Conservative")).toBeChecked();
    expect(within(card).getByLabelText("Aggressive")).toBeDisabled();
  });

  it("schedules a WLAN: its days and times, overnight", async () => {
    const user = userEvent.setup();
    config.site.timezone = "Europe/Berlin";
    await renderWifi();
    const row = await screen.findByRole("listitem", { name: "uniwrt-demo" });
    await user.click(within(row).getByRole("button", { name: "Edit uniwrt-demo" }));
    const editor = await screen.findByRole("dialog", { name: "WLAN uniwrt-demo" });
    const schedule = within(editor).getByRole("region", { name: "Schedule" });
    expect(within(schedule).getByText(/Europe\/Berlin time/)).toBeInTheDocument();
    await user.click(within(schedule).getByLabelText("Only on at certain times"));
    // A new time is every day from 07:00 to 22:00: the weekend goes, and it runs overnight.
    await user.click(within(schedule).getByRole("button", { name: "Saturday" }));
    await user.click(within(schedule).getByRole("button", { name: "Sunday" }));
    expect(within(schedule).getByRole("button", { name: "Sunday" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    fireEvent.input(within(schedule).getByLabelText("From"), { target: { value: "18:00" } });
    // A person's edits land in separate ticks: Solid flushes the first before the second.
    await new Promise((settled) => setTimeout(settled));
    fireEvent.input(within(schedule).getByLabelText("To"), { target: { value: "01:30" } });
    expect(await within(schedule).findByText("until the next day")).toBeInTheDocument();
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().config.wlans[0]?.schedule).toEqual({
      spans: [{ days: ["mon", "tue", "wed", "thu", "fri"], from: 1080, to: 90 }],
    });
  });

  it("shows the server's refusal on its field, and deletes a WLAN", async () => {
    const user = userEvent.setup();
    await renderWifi();
    const row = await screen.findByRole("listitem", { name: "uniwrt-demo" });
    await user.click(within(row).getByRole("button", { name: "Edit uniwrt-demo" }));
    const editor = await screen.findByRole("dialog", { name: "WLAN uniwrt-demo" });
    refusal = { path: "wlans/home/network", message: "There is no network lan" };
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText("There is no network lan")).toBeInTheDocument();
    await user.click(within(editor).getByRole("button", { name: "Delete" }));
    const confirm = within(editor)
      .getByText(/Delete uniwrt-demo\?/)
      .closest("div");
    if (!confirm) throw new Error("the confirmation has a box");
    await user.click(within(confirm).getByRole("button", { name: "Delete" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().config.wlans).toEqual([]);
    expect(put().message).toBe("Delete WLAN uniwrt-demo");
  });

  it("shows a QR code to join with, reading the passphrase from the vault", async () => {
    const user = userEvent.setup();
    await renderWifi();
    const row = await screen.findByRole("listitem", { name: "uniwrt-demo" });
    await user.click(within(row).getByRole("button", { name: "QR code uniwrt-demo" }));
    const dialog = await screen.findByRole("dialog", { name: "Join uniwrt-demo" });
    expect(await within(dialog).findByRole("img", { name: "Join uniwrt-demo" })).toBeVisible();
    expect(within(dialog).getByText("correct horse")).toBeInTheDocument();
    expect(calls.find((c) => c.url === "/api/v1/site/secrets/reveal")?.body).toEqual({
      path: "wlans/home/passphrase",
    });
  });
});

describe("Settings › Site", () => {
  it("edits the site's name, country and time zone", async () => {
    const user = userEvent.setup();
    await renderWifi("admin", "/settings");
    const card = await screen.findByRole("region", { name: "Site" }).catch(() => undefined);
    const scope = card ? within(card) : screen;
    const name = await scope.findByLabelText("Name");
    expect(name).toHaveValue("Demo home");
    expect(scope.getByLabelText("Country")).toHaveValue("US");
    expect(scope.getByRole("button", { name: "Save" })).toBeDisabled();
    await user.selectOptions(scope.getByLabelText("Country"), "DE");
    await user.selectOptions(scope.getByLabelText("Time zone"), "Europe/Berlin");
    await user.clear(name);
    await user.click(scope.getByRole("button", { name: "Save" }));
    expect(await scope.findByText("A name is 1 to 64 characters.")).toBeInTheDocument();
    await user.type(name, "Home");
    refusal = { path: "site/timezone", message: "Europe/Berlin is not a time zone" };
    await user.click(scope.getByRole("button", { name: "Save" }));
    expect(await scope.findByText("Europe/Berlin is not a time zone")).toBeInTheDocument();
    await user.click(scope.getByRole("button", { name: "Save" }));
    expect(await scope.findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().config.site).toMatchObject({
      name: "Home",
      country: "DE",
      timezone: "Europe/Berlin",
    });
    expect(put().message).toBe("Edit the site's settings");
  });

  it("is read-only for viewers", async () => {
    await renderWifi("viewer", "/settings");
    expect(await screen.findByLabelText("Country")).toBeDisabled();
    expect(screen.getByText("Only admins can change the site.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });
});

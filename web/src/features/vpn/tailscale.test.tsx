import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { SiteSave } from "../../api/gen/SiteSave";
import type { Tailscale } from "../../api/gen/Tailscale";
import type { TailscaleFacts } from "../../api/gen/TailscaleFacts";
import type { TailscaleStatus } from "../../api/gen/TailscaleStatus";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../test/site";

const KEY = "tskey-auth-k1234CNTRL-abcdef";
const URL_ = "https://login.tailscale.com/a/1a2b3c4d";
const SAVED_KEY = "config/00000000000000000000000000000007";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig;
let status: TailscaleStatus;

const tailscale: Tailscale = {
  enabled: true,
  routes: ["lan"],
  exit_node: true,
  hostname: null,
  auth_key: null,
};

function daemon(over: Partial<TailscaleFacts> = {}): TailscaleFacts {
  return {
    state: "running",
    version: "1.80.3-1 (OpenWrt)",
    tailnet: "robin@example.com",
    dns_suffix: "tail1234.ts.net",
    dns_name: "gw.tail1234.ts.net",
    addresses: ["100.101.102.103", "fd7a:115c:a1e0::1"],
    online: true,
    approved_routes: ["192.168.1.0/24"],
    health: [],
    peer_count: 3,
    peers: [
      {
        name: "laptop",
        dns_name: "laptop.tail1234.ts.net",
        os: "linux",
        addresses: ["100.64.0.2"],
        online: true,
        active: true,
        exit_node_option: false,
        direct: "198.51.100.7:41641",
        relay: "fra",
        last_seen: null,
        rx_bytes: 2_000_000,
        tx_bytes: 30_000,
      },
      {
        name: "phone",
        dns_name: null,
        os: "android",
        addresses: ["100.64.0.3"],
        online: false,
        active: false,
        exit_node_option: false,
        direct: null,
        relay: null,
        last_seen: Date.now() - 3_600_000,
        rx_bytes: 0,
        tx_bytes: 0,
      },
    ],
    prefs: {
      want_running: true,
      logged_out: false,
      advertise_routes: ["192.168.1.0/24", "0.0.0.0/0", "::/0"],
      hostname: null,
      accept_dns: false,
      accept_routes: false,
      netfilter_off: true,
    },
    ...over,
  };
}

const loggedOut = () =>
  daemon({
    state: "needs_login",
    tailnet: null,
    dns_name: null,
    addresses: [],
    online: false,
    approved_routes: [],
    peer_count: 0,
    peers: [],
    prefs: { ...(daemon().prefs as NonNullable<TailscaleFacts["prefs"]>), logged_out: true },
  });

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  status = {
    online: true,
    collected_at: Date.now() - 20_000,
    daemon: daemon(),
    prefs_applied: true,
    unapproved_routes: ["0.0.0.0/0"],
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head, config));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        const save = body as SiteSave;
        config = structuredClone(save.config);
        const key = save.secrets?.["vpn/tailscale/auth_key"];
        if (config.vpn.tailscale && key !== undefined) {
          config.vpn.tailscale.auth_key = key === null ? null : SAVED_KEY;
        }
        return Response.json(siteView(head, config));
      }
      if (url === "/api/v1/vpn/tailscale") return Response.json(status);
      if (url === "/api/v1/vpn/tailscale/login") {
        return Response.json({ state: "needs_login", auth_url: URL_ });
      }
      if (url === "/api/v1/vpn/tailscale/logout") {
        return Response.json({ state: "needs_login", auth_url: null });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderVpn(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/vpn"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gw", { role: "gateway", mode: "managed", version: "24.10.4" })]);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  await screen.findByRole("heading", { name: "VPN", level: 1 });
  return ws;
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;
const card = () => screen.findByRole("region", { name: "Tailscale" });

describe("Tailscale", () => {
  it("sets Tailscale up with an auth key", async () => {
    await renderVpn();
    const region = await card();
    expect(within(region).getByText(/Tailscale isn't set up/)).toBeInTheDocument();
    await userEvent.click(within(region).getByRole("button", { name: "Set up Tailscale" }));
    const dialog = await screen.findByRole("dialog", { name: "Set up Tailscale" });
    expect(within(dialog).getByLabelText("Default")).toBeChecked();
    await userEvent.click(within(dialog).getByLabelText("Offer the gateway as an exit node"));
    await userEvent.type(within(dialog).getByLabelText("Machine name"), "home-gw");
    await userEvent.click(within(dialog).getByRole("button", { name: "With an auth key" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText(/Paste an auth key/)).toBeInTheDocument();
    await userEvent.type(within(dialog).getByLabelText("Auth key"), KEY);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByText(/Saved as revision 3/);
    expect(put().config.vpn.tailscale).toEqual({
      enabled: true,
      routes: ["lan"],
      exit_node: true,
      hostname: "home-gw",
      auth_key: null,
    });
    expect(put().secrets).toEqual({ "vpn/tailscale/auth_key": KEY });
    expect(put().message).toBe("Set up Tailscale");
  });

  it("shows the gateway in its tailnet, what awaits approval and the other devices", async () => {
    config = siteConfig();
    config.vpn.tailscale = tailscale;
    await renderVpn("viewer");
    const region = await card();
    await within(region).findByText("Connected");
    expect(region).toHaveTextContent("robin@example.com");
    expect(region).toHaveTextContent("gw.tail1234.ts.net");
    expect(region).toHaveTextContent("100.101.102.103");
    expect(region).toHaveTextContent(/Waiting for approval .*: the exit node/);
    expect(within(region).getByRole("link", { name: /admin console/ })).toHaveAttribute(
      "href",
      "https://login.tailscale.com/admin/machines",
    );
    expect(region).toHaveTextContent(/24.10 sends the daemon's logs/);
    expect(within(region).queryByRole("button", { name: "Log out" })).toBeNull();
    const peers = screen.getByRole("region", { name: "Tailnet devices" });
    expect(within(peers).getByRole("listitem", { name: "laptop" })).toHaveTextContent(
      "Direct · 198.51.100.7:41641",
    );
    expect(within(peers).getByRole("listitem", { name: "phone" })).toHaveTextContent(/Seen 1/);
    expect(peers).toHaveTextContent("Showing 2 of 3.");
  });

  it("logs the gateway in through the URL it hands out", async () => {
    config = siteConfig();
    config.vpn.tailscale = tailscale;
    status = { ...status, daemon: loggedOut(), unapproved_routes: [] };
    await renderVpn();
    const region = await card();
    await within(region).findByText(/Logged out: log in/);
    expect(screen.queryByRole("region", { name: "Tailnet devices" })).toBeNull();
    await userEvent.click(within(region).getByRole("button", { name: "Log in" }));
    const dialog = await screen.findByRole("dialog", { name: "Log in to Tailscale" });
    const link = await within(dialog).findByRole("link", { name: /Open the login page/ });
    expect(link).toHaveAttribute("href", URL_);
    expect(within(dialog).getByRole("img", { name: "QR code of the login link" })).toBeVisible();
    expect(calls.some((c) => c.method === "POST" && c.url === "/api/v1/vpn/tailscale/login")).toBe(
      true,
    );
  });

  it("logs the gateway out after a confirmation", async () => {
    config = siteConfig();
    config.vpn.tailscale = { ...tailscale, auth_key: SAVED_KEY };
    await renderVpn();
    const region = await card();
    await userEvent.click(await within(region).findByRole("button", { name: "Log out" }));
    const alert = within(region).getByRole("alert");
    expect(alert).toHaveTextContent(/logs it in again with the saved auth key/);
    await userEvent.click(within(alert).getByRole("button", { name: "Log out" }));
    await waitFor(() =>
      expect(
        calls.some((c) => c.method === "POST" && c.url === "/api/v1/vpn/tailscale/logout"),
      ).toBe(true),
    );
  });

  it("clears the auth key for the URL and removes Tailscale with its zone's policies", async () => {
    config = siteConfig();
    config.vpn.tailscale = { ...tailscale, auth_key: SAVED_KEY };
    config.firewall.policies = [
      ...config.firewall.policies,
      {
        ...(config.firewall.policies[0] as SiteConfig["firewall"]["policies"][number]),
        from: "tailscale",
      },
    ].filter(Boolean);
    const policies = config.firewall.policies.length;
    await renderVpn();
    const region = await card();
    await userEvent.click(within(region).getByRole("button", { name: "Edit" }));
    let dialog = await screen.findByRole("dialog", { name: "Tailscale" });
    expect(within(dialog).getByText(/A key is saved/)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "From this page" }));
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByText(/Saved as revision 3/);
    expect(put().secrets).toEqual({ "vpn/tailscale/auth_key": null });
    await userEvent.click(
      within(dialog).getAllByRole("button", { name: "Close" }).at(-1) as HTMLElement,
    );

    await userEvent.click(within(await card()).getByRole("button", { name: "Edit" }));
    dialog = await screen.findByRole("dialog", { name: "Tailscale" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove Tailscale" }));
    await userEvent.click(
      within(within(dialog).getByRole("alert")).getByRole("button", { name: "Remove Tailscale" }),
    );
    await within(dialog).findByText(/Saved as revision 4/);
    expect(put().config.vpn.tailscale).toBeNull();
    expect(put().secrets).toBeUndefined();
    expect(put().config.firewall.policies.some((p) => p.from === "tailscale")).toBe(false);
    expect(put().config.firewall.policies.length).toBe(policies - 1);
  });
});

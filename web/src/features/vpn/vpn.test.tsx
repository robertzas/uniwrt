import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { SiteSave } from "../../api/gen/SiteSave";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { VpnStatus } from "../../api/gen/VpnStatus";
import type { WireGuard } from "../../api/gen/WireGuard";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../test/site";

const KEY = `${"A".repeat(42)}E=`;
const SERVER_KEY = `${"S".repeat(42)}E=`;
const NOW = Date.now();

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig = siteConfig();
let status: VpnStatus;

const phone: VpnPeer = {
  id: "phone",
  name: "Robin's phone",
  enabled: true,
  address: "10.0.250.2",
  public_key: KEY,
  private_key: "config/00000000000000000000000000000002",
  preshared_key: "config/00000000000000000000000000000003",
  role: { kind: "client", full_tunnel: true },
};

const cabin: VpnPeer = {
  ...phone,
  id: "cabin",
  name: "Cabin",
  address: "10.0.250.3",
  public_key: `${"C".repeat(42)}E=`,
  private_key: null,
  role: { kind: "site", networks: ["192.168.50.0/24"], endpoint: null },
};

function server(peers: VpnPeer[]): WireGuard {
  return {
    enabled: true,
    port: 51820,
    address: "10.0.250.1/24",
    endpoint: { kind: "ddns", id: "home" },
    dns: true,
    private_key: "config/00000000000000000000000000000001",
    public_key: SERVER_KEY,
    peers,
  };
}

/** What the save's keys make: the server's keypair, each new peer's and its preshared key. */
function filled(saved: SiteConfig): SiteConfig {
  const wg = saved.vpn.wireguard;
  if (!wg) return saved;
  return {
    ...saved,
    vpn: {
      ...saved.vpn,
      wireguard: {
        ...wg,
        private_key: wg.private_key ?? "config/0000000000000000000000000000000a",
        public_key: wg.public_key ?? SERVER_KEY,
        peers: wg.peers.map((p) => ({
          ...p,
          public_key: p.public_key || `${"N".repeat(42)}E=`,
          private_key:
            p.private_key ?? (p.public_key ? null : "config/0000000000000000000000000000000b"),
          preshared_key: p.preshared_key ?? "config/0000000000000000000000000000000c",
        })),
      },
    },
  };
}

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  config.services.ddns = [
    {
      id: "home",
      enabled: true,
      hostname: "home.example.com",
      provider: { kind: "duckdns" },
      ipv6: false,
      source: "wan",
      secret: "config/00000000000000000000000000000009",
    },
  ];
  status = {
    online: true,
    collected_at: NOW - 20_000,
    running: true,
    key_applied: true,
    peers: [
      {
        id: "phone",
        on_gateway: true,
        connected: true,
        handshake_at: NOW - 65_000,
        endpoint: "203.0.113.9:4500",
        rx_bytes: 1_500_000,
        tx_bytes: 20_000,
      },
      {
        id: "cabin",
        on_gateway: false,
        connected: false,
        handshake_at: null,
        endpoint: null,
        rx_bytes: 0,
        tx_bytes: 0,
      },
    ],
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
        config = filled((body as SiteSave).config);
        return Response.json(siteView(head, config));
      }
      if (url === "/api/v1/vpn/wireguard/peers") return Response.json(status);
      const conf = url.match(/^\/api\/v1\/vpn\/wireguard\/peers\/(\w+)\/config$/);
      if (conf && method === "POST") {
        return Response.json({
          file_name: `${conf[1]}.conf`,
          conf: `[Interface]\nPrivateKey = secret\nAddress = 10.0.250.4/32\n`,
          own_key: false,
        });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderVpn(role: "admin" | "viewer" = "admin", path = "/vpn") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(path));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gw", { role: "gateway" })]);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  if (path === "/vpn") await screen.findByRole("heading", { name: "VPN", level: 1 });
  return ws;
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("VPN", () => {
  it("sets up a WireGuard server dialed at the DDNS name", async () => {
    const ws = await renderVpn();
    const card = await screen.findByRole("region", { name: "WireGuard server" });
    expect(within(card).getByText(/No WireGuard server yet/)).toBeInTheDocument();
    await userEvent.click(within(card).getByRole("button", { name: "Set up WireGuard" }));
    const dialog = await screen.findByRole("dialog", { name: "Set up WireGuard" });
    expect(within(dialog).getByLabelText("DDNS name")).toHaveValue("home");
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByText(/Saved as revision 3/);
    // The controller announces the new head, as after any save: the editor stays open.
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    await waitFor(() => expect(screen.getByText("home.example.com:51820")).toBeInTheDocument());
    expect(within(dialog).getByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().config.vpn.wireguard).toEqual({
      enabled: true,
      port: 51820,
      address: "10.0.250.1/24",
      endpoint: { kind: "ddns", id: "home" },
      dns: true,
      private_key: null,
      public_key: null,
      peers: [],
    });
    await userEvent.click(
      within(dialog).getAllByRole("button", { name: "Close" }).at(-1) as HTMLElement,
    );
    // The card comes back with the tunnel's status.
    const region = () => screen.getByRole("region", { name: "WireGuard server" });
    await waitFor(() => expect(region()).toHaveTextContent("home.example.com:51820"));
    expect(within(region()).getByText(SERVER_KEY)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Peers" })).toHaveTextContent("No peers yet.");
  });

  it("shows the peers with what the gateway reports, to viewers too", async () => {
    config.vpn.wireguard = server([phone, cabin]);
    await renderVpn("viewer");
    const card = await screen.findByRole("region", { name: "WireGuard server" });
    expect(await within(card).findByText("Running")).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    const peers = screen.getByRole("region", { name: "Peers" });
    const row = within(peers).getByRole("listitem", { name: "Robin's phone" });
    expect(row).toHaveTextContent("10.0.250.2 · Client · full tunnel");
    expect(await within(row).findByText(/^Connected · handshake 1m \d+s ago$/)).toBeInTheDocument();
    expect(row).toHaveTextContent("203.0.113.9:4500 · ↓ 1.5 MB · ↑ 20 KB");
    const site = within(peers).getByRole("listitem", { name: "Cabin" });
    expect(site).toHaveTextContent("Site · 192.168.50.0/24");
    expect(site).toHaveTextContent("Own key");
    expect(site).toHaveTextContent("Not on the gateway yet: apply");
    expect(
      within(peers).queryByRole("button", { name: /Configuration of/ }),
    ).not.toBeInTheDocument();
  });

  it("adds a peer and shows its configuration", async () => {
    config.vpn.wireguard = server([phone, cabin]);
    await renderVpn();
    const peers = await screen.findByRole("region", { name: "Peers" });
    await userEvent.click(within(peers).getByRole("button", { name: "Add peer" }));
    const dialog = await screen.findByRole("dialog", { name: "Add peer" });
    expect(within(dialog).getByLabelText("Address in the tunnel")).toHaveValue("10.0.250.4");
    await userEvent.type(within(dialog).getByLabelText("Name"), "Laptop");
    await userEvent.click(within(dialog).getByRole("button", { name: /Only your networks/ }));
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Save and show configuration" }),
    );
    const shown = await screen.findByRole("dialog", { name: "Configuration of Laptop" });
    expect(put().config.vpn.wireguard?.peers.at(-1)).toEqual({
      id: "laptop",
      name: "Laptop",
      enabled: true,
      address: "10.0.250.4",
      public_key: "",
      private_key: null,
      preshared_key: null,
      role: { kind: "client", full_tunnel: false },
    });
    expect(put().message).toBe("New WireGuard peer Laptop");
    expect(
      await within(shown).findByRole("img", { name: "QR code of Laptop's configuration" }),
    ).toBeInTheDocument();
    expect(within(shown).getByText(/PrivateKey = secret/)).toBeInTheDocument();
    expect(within(shown).getByText("laptop.conf")).toBeInTheDocument();
    expect(calls.some((c) => c.method === "POST" && c.url.endsWith("/peers/laptop/config"))).toBe(
      true,
    );
  });

  it("refuses a site whose networks overlap a local one, then deletes a peer", async () => {
    config.vpn.wireguard = server([phone]);
    const lan = config.networks[0]?.ipv4?.address ?? "";
    await renderVpn();
    const peers = await screen.findByRole("region", { name: "Peers" });
    await userEvent.click(within(peers).getByRole("button", { name: "Add peer" }));
    const dialog = await screen.findByRole("dialog", { name: "Add peer" });
    await userEvent.type(within(dialog).getByLabelText("Name"), "Office");
    await userEvent.click(within(dialog).getByRole("radio", { name: /Another site/ }));
    expect(
      [...dialog.querySelectorAll("fieldset label")].map((l) =>
        l.className.includes("bg-accent/10"),
      ),
    ).toEqual([false, true]);
    const subnet = lan.replace(/\.\d+\//, ".0/");
    await userEvent.type(within(dialog).getByLabelText("Its networks"), subnet);
    await userEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText(/overlaps .*'s subnet/)).toBeInTheDocument();
    expect(put()).toBeUndefined();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await userEvent.click(within(peers).getByRole("button", { name: "Edit Robin's phone" }));
    const edit = await screen.findByRole("dialog", { name: "Edit Robin's phone" });
    await userEvent.click(within(edit).getByRole("button", { name: "Delete" }));
    await userEvent.click(
      within(within(edit).getByRole("alert")).getByRole("button", { name: "Delete" }),
    );
    await within(edit).findByText(/Saved as revision 3/);
    expect(put().config.vpn.wireguard?.peers).toEqual([]);
  });

  it("adds the tunnel's zone to the firewall matrix", async () => {
    config.vpn.wireguard = server([]);
    await renderVpn("admin", "/security");
    const matrix = await screen.findByRole("table", { name: "Traffic between networks" });
    expect(within(matrix).getByRole("rowheader", { name: "WireGuard VPN" })).toBeInTheDocument();
    expect(within(matrix).getByText("WireGuard VPN to Internet: Allow")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByText(/WireGuard and Tailscale peers/)).toBeInTheDocument(),
    );
  });
});

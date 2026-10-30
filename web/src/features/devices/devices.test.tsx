import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeFacts } from "../../api/gen/NodeFacts";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { detail, facts, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

function renderAt(path: string) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  return { ws, history };
}

/** A gateway with one 5 GHz radio, a client named by its lease, and a mesh peer. */
function wirelessFacts(): NodeFacts {
  const f = facts(12);
  f.radios = [
    {
      name: "radio1",
      phy: "phy1",
      band: "5g",
      up: true,
      disabled: false,
      channel: 36,
      frequency_mhz: 5180,
      center_frequency_mhz: 5210,
      width: 80,
      standard: "he",
      tx_power_dbm: 23,
      noise_dbm: -92,
      channel_busy_percent: 18,
      country: "US",
      ifaces: [
        {
          name: "phy1-ap0",
          section: "default_radio1",
          mode: "ap",
          ssid: "Home",
          mesh_id: null,
          bssid: "02:00:00:00:01:01",
          encryption: "sae-mixed",
          networks: ["lan"],
          up: true,
        },
      ],
    },
  ];
  const station = {
    iface: "phy1-ap0",
    signal_dbm: -58,
    connected_s: 3720,
    inactive_ms: 10,
    rx_rate: null,
    tx_rate: null,
    rx_bytes: 1_000_000,
    tx_bytes: 4_000_000,
    rx_packets: 1,
    tx_packets: 1,
    tx_retries: null,
    tx_failed: null,
    expected_throughput_kbps: null,
    rx_airtime_us: null,
    tx_airtime_us: null,
    peer_link: null,
    authorized: true,
    standard: null,
    capabilities: [],
  };
  f.stations = [
    { ...station, mac: "aa:00:00:00:00:01" },
    { ...station, mac: "02:00:00:00:00:02", peer_link: "established" },
  ];
  f.leases = [
    {
      ip: "192.168.1.50",
      mac: "aa:00:00:00:00:01",
      hostname: "laptop",
      client_id: null,
      device: "br-lan",
      expires_in_s: 3600,
    },
  ];
  return f;
}

describe("Devices", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" }),
      ),
    );
  });

  it("lists the fleet live, gateway first, and opens a device", async () => {
    const user = userEvent.setup();
    const { ws, history } = renderAt("/devices");
    expect(await screen.findByText("Loading devices…")).toBeInTheDocument();

    sendFleet(ws, [
      summary(2, "attic", { cpu_percent: 7 }),
      summary(1, "gateway", { role: "gateway" }),
    ]);
    const table = await screen.findByRole("table", { name: "Devices" });
    const rows = () => within(table).getAllByRole("row").slice(1);
    expect(rows().map((row) => row.querySelector("td")?.textContent)).toEqual([
      "gatewayGateway",
      "atticAccess point",
    ]);
    expect(rows()[1]).toHaveTextContent("7 %");

    ws.receive({
      type: "fleet_delta",
      topic: "fleet",
      nodes: [
        summary(2, "attic", {
          cpu_percent: 9,
          status: { state: "offline", since: 1, reasons: [{ kind: "no_agent" }] },
        }),
      ],
      removed: [],
    });
    await waitFor(() => expect(rows()[1]).toHaveTextContent("9 %"));
    expect(rows()[1]).toHaveTextContent("Offline");

    await user.click(within(table).getByText("attic"));
    await waitFor(() => expect(history.get()).toBe("/devices/2"));
  });

  it("shows an empty fleet", async () => {
    const { ws } = renderAt("/devices");
    sendFleet(ws, []);
    await screen.findByRole("heading", { name: "No devices yet" });
  });

  it("shows a device's stats, radios and clients, and follows its deltas", async () => {
    const { ws } = renderAt("/devices/1");
    await waitFor(() => expect(ws.sent.at(-1)).toEqual({ sub: ["fleet", "node:1"] }));

    ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(wirelessFacts()) });
    await screen.findByRole("heading", { name: "gw" });
    expect(document.title).toBe("gw · uniwrt");
    expect(screen.getByTitle(/^Read-only/)).toHaveTextContent("Observe");
    expect(screen.getByText("5 GHz")).toBeInTheDocument();
    expect(screen.getByText("Channel 36 · 80 MHz · Wi-Fi 6")).toBeInTheDocument();
    expect(screen.getByText("Home")).toBeInTheDocument();
    expect(screen.getByText("1 mesh peer")).toBeInTheDocument();

    const clients = screen.getByRole("table", { name: "Wi-Fi clients" });
    const [client] = within(clients).getAllByRole("row").slice(1);
    expect(client).toHaveTextContent("laptop");
    expect(within(client as HTMLElement).getByTitle("aa:00:00:00:00:01")).toHaveTextContent(
      "laptop",
    );
    expect(client).toHaveTextContent("192.168.1.50");
    expect(client).toHaveTextContent("Home · 5 GHz");
    expect(client).toHaveTextContent("-58 dBm");
    expect(client).toHaveTextContent("1h 2m");
    expect(client).toHaveTextContent("5 MB");

    ws.receive({
      type: "node_delta",
      topic: "node:1",
      status: {
        state: "degraded",
        since: 2,
        reasons: [{ kind: "collect_failed", set: "stations", error: "timeout" }],
      },
      events: [],
      roams: [],
      removed: false,
    });
    expect(await screen.findByText("Collecting stations failed: timeout")).toBeInTheDocument();
    expect(screen.getByText("Degraded")).toBeInTheDocument();
  });

  it("says when a device does not exist", async () => {
    const { ws } = renderAt("/devices/9");
    await waitFor(() => expect(ws.sent.at(-1)).toEqual({ sub: ["fleet", "node:9"] }));
    ws.receive({ type: "error", topic: "node:9", message: "no node 9" });
    await screen.findByRole("heading", { name: "Device not found" });
  });

  it("names an access point's clients from the gateway's leases", async () => {
    const gateway = detail(wirelessFacts());
    const fetch = vi.fn(async (url: string) =>
      url === "/api/v1/nodes/1"
        ? Response.json(gateway)
        : Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" }),
    );
    vi.stubGlobal("fetch", fetch);
    const { ws } = renderAt("/devices/2");
    sendFleet(ws, [summary(1, "gw", { role: "gateway" }), summary(2, "attic")]);
    const ap = detail({ ...wirelessFacts(), leases: [] });
    ap.node = { ...ap.node, id: 2, name: "attic", role: "ap" };
    ws.receive({ type: "node_snapshot", topic: "node:2", node: ap });

    const clients = await screen.findByRole("table", { name: "Wi-Fi clients" });
    await waitFor(() => expect(clients).toHaveTextContent("laptop"));
    expect(fetch).toHaveBeenCalledWith("/api/v1/nodes/1", expect.anything());
    // The gateway's topic stays unsubscribed: its focus is not held.
    expect(ws.sent.at(-1)).toEqual({ sub: ["fleet", "node:2"] });
  });
});

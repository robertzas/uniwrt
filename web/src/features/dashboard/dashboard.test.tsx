import { memoryHistory } from "@solidjs/router";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { createAppRouter } from "../../app/routes";
import { client } from "../../test/clients";
import { alertRecord, detail, facts, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { rfView } from "../../test/rf";
import { edge, house } from "../../test/topology";

let calls: string[] = [];

function stubApi() {
  calls = [];
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const key = `${init?.method ?? "GET"} ${url}`;
    calls.push(key);
    if (key === "GET /api/v1/nodes/1") {
      const withWan = facts(10);
      withWan.interfaces = [
        {
          name: "wan",
          proto: "dhcp",
          up: true,
          device: "eth1",
          uptime_s: 7_200,
          ipv4: ["203.0.113.7/24"],
          ipv6: [],
          delegated_prefixes: ["2001:db8:1::/56"],
          ipv4_gateway: "203.0.113.1",
          ipv6_gateway: null,
          dns_servers: ["1.1.1.1"],
          errors: [],
        },
      ];
      return Response.json(detail(withWan));
    }
    if (key === "GET /api/v1/wan/speed-tests?limit=1") {
      return Response.json({
        tests: [
          {
            job: 9,
            node: 1,
            at: 1_000,
            bypassed: false,
            download_kbps: 450_000,
            upload_kbps: 90_000,
            idle_us: 9_000,
            download_us: 11_000,
            upload_us: 12_000,
            grade: "A+",
          },
        ],
        suggestion: null,
      });
    }
    if (key === "GET /api/v1/topology/layout") return Response.json([]);
    if (key === "GET /api/v1/logs/patterns") return Response.json({ patterns: [] });
    if (key === "GET /api/v1/topology/timeline") return Response.json({ start: 0, changes: [] });
    if (key.startsWith("GET /api/v1/metrics?")) {
      const empty: MetricsResponse = {
        entity: "site",
        from: 0,
        to: 1,
        resolution: "raw",
        series: [],
      };
      return Response.json(empty);
    }
    return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
  });
  vi.stubGlobal("fetch", fetch);
}

const fleet = [summary(1, "Gateway", { role: "gateway" }), summary(2, "Living room")];

/** Renders `/` with the fleet, and sends the topology and clients once subscribed. */
async function renderDashboard(nodes: NodeSummary[] = fleet) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory("/");
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, nodes);
  if (nodes.length === 0) return { ws, history };
  await waitFor(() =>
    expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["topology", "clients"]) }),
  );
  const topology = house();
  topology.edges[0] = edge("internet", "node:1", {
    medium: "wan",
    b_port: "eth1",
    traffic: { at: 1, down_bytes: 0, up_bytes: 0, down_bps: 48_000_000, up_bps: 2_000_000 },
  });
  ws.receive({ type: "topology_snapshot", topic: "topology", topology });
  const busy = client("aa:00:00:00:00:01", {
    hostname: "Annas-iPhone",
    experience: { score: 92, parts: [] },
  });
  const base = busy.connection!;
  ws.receive({
    type: "clients_snapshot",
    topic: "clients",
    clients: [
      {
        ...busy,
        connection: {
          ...base,
          traffic: { at: 1, down_bytes: 0, up_bytes: 0, down_bps: 30_000_000, up_bps: 1_000_000 },
        },
      },
      client("aa:00:00:00:00:02", {
        hostname: "Laptop",
        kind: "laptop",
        experience: { score: 45, parts: [] },
      }),
    ],
  });
  return { ws, history };
}

describe("the dashboard", () => {
  beforeEach(stubApi);

  it("shows the network's health at a glance", async () => {
    await renderDashboard();
    const health = await screen.findByRole("region", { name: "Network health" });
    const tile = (name: RegExp) => within(health).getByRole("link", { name });
    await waitFor(() => expect(tile(/Internet/)).toHaveTextContent("Online"));
    expect(tile(/Internet/)).toHaveTextContent("↓ 48 Mb/s · ↑ 2 Mb/s");
    expect(tile(/Mesh/)).toHaveTextContent("1 link");
    expect(tile(/Mesh/)).toHaveTextContent("Weakest link at 75/100");
    // Two Wi-Fi clients scored 92 and 45: fair on average.
    expect(tile(/Wi-Fi experience/)).toHaveTextContent("69");
    expect(tile(/Nodes/)).toHaveTextContent("2/2");
    expect(tile(/Nodes/)).toHaveAttribute("href", "/devices");
    expect(tile(/Clients online/)).toHaveTextContent("2");
  });

  it("shows the internet connection's addresses, read from the gateway's record", async () => {
    await renderDashboard();
    await waitFor(() => expect(calls).toContain("GET /api/v1/nodes/1"));
    expect(await screen.findByText("203.0.113.7/24")).toBeInTheDocument();
    expect(screen.getByText("2001:db8:1::/56")).toBeInTheDocument();
    expect(await screen.findByText("A+ · 450 Mb/s ↓ 90 Mb/s ↑")).toBeInTheDocument();
    expect(screen.getByText("203.0.113.1")).toBeInTheDocument();
    expect(screen.getByText("2h")).toBeInTheDocument();
    expect(calls.some((call) => call.includes("/api/v1/metrics?entity=site"))).toBe(true);
    // The gateway's record is read, never followed: following it would collect it more often.
    expect(calls.filter((call) => call === "GET /api/v1/nodes/1")).toHaveLength(1);
  });

  it("ranks the busiest clients and scores Wi-Fi experience", async () => {
    await renderDashboard();
    const top = await screen.findByRole("region", { name: "Top clients" });
    const rows = await within(top).findAllByRole("listitem");
    // The laptop moves the fixture's 2.4 Mb/s: second.
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("Annas-iPhone"),
      expect.stringContaining("Laptop"),
    ]);
    expect(rows[0]).toHaveTextContent("Download 30 Mb/s, upload 1 Mb/s");
    expect(within(rows[0]!).getByRole("link")).toHaveAttribute(
      "href",
      "/clients/aa:00:00:00:00:01",
    );
    const experience = screen.getByRole("region", { name: "Wi-Fi experience" });
    expect(within(experience).getByRole("img", { name: "Experience score 69 of 100" }));
    expect(experience).toHaveTextContent("Weakest access point");
  });

  it("draws the mini-map and opens a device from it", async () => {
    const { history } = await renderDashboard();
    const map = await screen.findByRole("img", { name: /^Network map: / });
    expect(map).toHaveAccessibleName("Network map: 2 devices and 3 clients");
    expect(map).not.toHaveAttribute("tabindex");
    fireEvent.click(screen.getByRole("link", { name: /Open the map/ }));
    await waitFor(() => expect(history.get()).toBe("/topology"));
  });

  it("lists the recent events, live, newest first", async () => {
    const { ws } = await renderDashboard();
    await waitFor(() =>
      expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["events?min=notice"]) }),
    );
    const base = {
      count: 1,
      category: "system" as const,
      client: null,
      condition: null,
    };
    ws.receive({
      type: "events_snapshot",
      topic: "events?min=notice",
      events: [
        {
          ...base,
          id: 1,
          at: Date.now() - 5_000,
          last_at: Date.now() - 5_000,
          level: "notice",
          node: 2,
          what: { kind: "node_adopted" },
        },
      ],
    });
    const card = await screen.findByRole("region", { name: "Recent events" });
    await within(card).findByText(/adopted/);
    ws.receive({
      type: "events_delta",
      topic: "events?min=notice",
      events: [
        {
          ...base,
          id: 2,
          at: Date.now(),
          last_at: Date.now(),
          level: "critical",
          node: 1,
          category: "wan",
          what: { kind: "wan_down", device: "eth1" },
        },
      ],
    });
    await waitFor(() => {
      const rows = within(card).getAllByRole("listitem");
      expect(rows[0]).toHaveTextContent(/Internet down on .* \(eth1\)/);
      expect(rows).toHaveLength(2);
    });
    expect(within(card).getAllByRole("link")[1]).toHaveAttribute(
      "href",
      "/insights/events?nodes=1",
    );
  });

  it("puts the alerts firing above the recent events", async () => {
    const { ws } = await renderDashboard();
    const card = await screen.findByRole("region", { name: "Recent events" });
    ws.receive({
      type: "alerts_snapshot",
      topic: "alerts",
      alerts: [alertRecord(1), alertRecord(2, { acknowledged: { by: 1, username: "a", at: 1 } })],
    });
    const firing = await within(card).findByRole("region", { name: "Firing" });
    const rows = within(firing).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent(/Device CPU busy.*CPU 93 %, above 90 %.*Firing/);
    expect(rows[1]).toHaveTextContent("Acknowledged");
    expect(within(firing).getByRole("link", { name: "2 alerts" })).toHaveAttribute(
      "href",
      "/insights/alerts",
    );
    ws.receive({
      type: "alerts_delta",
      topic: "alerts",
      alerts: [1, 2].map((id) => alertRecord(id, { resolved_at: Date.now() })),
    });
    await waitFor(() => expect(within(card).queryByRole("region", { name: "Firing" })).toBeNull());
  });

  it("sums up each band's air and links to RF insight", async () => {
    const { ws } = await renderDashboard();
    const card = await screen.findByRole("region", { name: "RF" });
    expect(within(card).getByRole("status")).toHaveTextContent("Reading what the radios hear");
    await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["rf"]) }));
    ws.receive({ type: "rf", topic: "rf", view: rfView() });
    const bands = await within(card).findAllByRole("listitem");
    expect(bands).toHaveLength(2);
    expect(bands[0]).toHaveTextContent(/2\.4 GHz.*Channel 1/);
    expect(bands[1]).toHaveTextContent(/5 GHz.*Channel 36, Channel 149/);
    expect(bands[1]).toHaveTextContent(/42 %.*Busiest channel · Gateway/);
    expect(bands[1]).toHaveTextContent("2 radios");
    expect(bands[1]).toHaveTextContent("1 neighbor");
    expect(bands[1]).toHaveTextContent("1 crowded");
    expect(bands[1]).toHaveTextContent("1 lookalike of your network");
    expect(within(bands[0] as HTMLElement).getByRole("link", { name: "2.4 GHz" })).toHaveAttribute(
      "href",
      "/insights/rf?band=2g",
    );
    expect(within(card).getByRole("link", { name: "Open RF insight" })).toHaveAttribute(
      "href",
      "/insights/rf",
    );
  });

  it("welcomes a new install with the wizard, or a way to adopt a running network", async () => {
    await renderDashboard([]);
    const setup = await screen.findByRole("link", { name: "Set up your internet router" });
    expect(setup).toHaveAttribute("href", "/setup");
    const adopt = screen.getByRole("link", { name: "Adopt an existing network" });
    expect(adopt).toHaveAttribute("href", "/devices?adopt=1");
    expect(screen.getByText("Welcome to uniwrt")).toBeInTheDocument();
  });
});

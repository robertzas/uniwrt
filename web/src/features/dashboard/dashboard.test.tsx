import { memoryHistory } from "@solidjs/router";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { createAppRouter } from "../../app/routes";
import { client } from "../../test/clients";
import { detail, facts, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
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
    if (key === "GET /api/v1/topology/layout") return Response.json([]);
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

  it("welcomes a new install with a way to adopt", async () => {
    await renderDashboard([]);
    const adopt = await screen.findByRole("link", { name: "Adopt a device" });
    expect(adopt).toHaveAttribute("href", "/devices?adopt=1");
    expect(screen.getByText("Welcome to uniwrt")).toBeInTheDocument();
  });
});

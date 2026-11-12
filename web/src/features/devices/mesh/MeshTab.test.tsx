import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BackhaulFailure } from "../../../api/gen/BackhaulFailure";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { metricsAnswer } from "../../../test/metrics";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { chain } from "../../../test/topology";

let calls: { method: string; url: string }[] = [];
let failures: BackhaulFailure[] = [];

beforeEach(() => {
  calls = [];
  failures = [
    {
      ends: [
        { node: 3, port: "eth0" },
        { node: 2, port: "eth1" },
      ],
      job: 7,
      at: 1_000,
      why: "No answer at 1532 bytes",
    },
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      calls.push({ method, url });
      if (url === "/api/v1/backhaul/failures") return Response.json(failures);
      if (url.startsWith("/api/v1/backhaul/failures/") && method === "DELETE") {
        failures = [];
        return new Response(null, { status: 204 });
      }
      if (url.startsWith("/api/v1/metrics?")) return Response.json(metricsAnswer(url, 70));
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

/** Access point 2 of the chain: meshed to the gateway, ap3 hanging from it. */
async function open(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/2?tab=mesh"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gw", { role: "gateway" }), summary(2, "ap"), summary(3, "ap3")]);
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["node:2"]) }));
  const ap = detail(facts(1));
  ws.receive({
    type: "node_snapshot",
    topic: "node:2",
    node: { ...ap, node: { ...ap.node, id: 2, name: "ap", role: "ap" } },
  });
  await waitFor(() =>
    expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["topology"]) }),
  );
  ws.receive({ type: "topology_snapshot", topic: "topology", topology: chain() });
  return ws;
}

describe("Device › Mesh", () => {
  it("shows the path to the gateway, what goes down with it, and its links", async () => {
    await open();
    const path = await screen.findByRole("region", { name: "Path to the gateway" });
    expect(within(path).getByText("1 hop, 1 over the air")).toBeInTheDocument();
    expect(within(path).getByText("Only the gateway")).toBeInTheDocument();
    expect(within(path).getByText("1 device and 3 clients lose the gateway")).toBeInTheDocument();
    expect(within(path).getByRole("link", { name: "ap3" })).toHaveAttribute(
      "href",
      "/devices/3?tab=mesh",
    );

    const links = screen.getByRole("table", { name: "Mesh links" });
    const rows = within(links).getAllByRole("row").slice(1);
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
      "02:00:00:00:00:99",
      "gw",
      "ap3",
    ]);
    const gw = within(links).getByRole("row", { name: "gw" });
    expect(within(gw).getByText("55 / 100, limited by signal")).toBeInTheDocument();
    expect(within(gw).getByText("Critical link")).toBeInTheDocument();
    expect(within(gw).getByText(/^2, last at/)).toBeInTheDocument();
    expect(
      await within(gw).findByRole("img", { name: "Quality of the link to gw, last 24 hours" }),
    ).toBeInTheDocument();
    expect(
      calls.some(
        (c) => new URL(c.url, "http://test").searchParams.get("entity") === "link:node:1~node:2",
      ),
    ).toBe(true);
    const peer = within(links).getByRole("row", { name: "02:00:00:00:00:99" });
    expect(within(peer).getByText("Down")).toBeInTheDocument();
  });

  it("lists the failed cables and lets an admin forget one", async () => {
    const user = userEvent.setup();
    await open();
    const card = await screen.findByRole("region", { name: "Failed cables" });
    const row = await within(card).findByRole("row", { name: "eth1" });
    expect(within(row).getByText("No answer at 1532 bytes")).toBeInTheDocument();
    expect(within(row).getByRole("link", { name: "ap3" })).toBeInTheDocument();
    await user.click(
      within(row).getByRole("button", { name: "Forget the failed probe of eth1 to ap3" }),
    );
    expect(
      await within(card).findByText("Forgotten: the next apply probes the cable again."),
    ).toBeInTheDocument();
    expect(calls).toContainEqual({
      method: "DELETE",
      url: "/api/v1/backhaul/failures/3/eth0/2/eth1",
    });
    expect(within(card).queryByRole("row", { name: "eth1" })).not.toBeInTheDocument();
  });

  it("shows viewers the failed cables without the forget button", async () => {
    await open("viewer");
    const card = await screen.findByRole("region", { name: "Failed cables" });
    await within(card).findByRole("row", { name: "eth1" });
    expect(within(card).queryByRole("button")).not.toBeInTheDocument();
  });
});

import { memoryHistory } from "@solidjs/router";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Topology } from "../../api/gen/Topology";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { house } from "../../test/topology";

let calls: string[] = [];

function stubApi() {
  calls = [];
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const key = `${init?.method ?? "GET"} ${url}`;
    calls.push(key);
    if (key === "GET /api/v1/topology/layout")
      return Response.json([{ vertex: "node:2", x: 5, y: 6 }]);
    if (key === "GET /api/v1/topology/timeline")
      return Response.json({ start: 500, changes: [800] });
    if (key.startsWith("GET /api/v1/topology/at/")) {
      const past: Topology = {
        ...house(),
        at: 700,
        nodes: house().nodes.slice(0, 3),
        edges: house().edges.slice(0, 2),
      };
      return Response.json(past);
    }
    if (init?.method === "PUT" || init?.method === "DELETE")
      return new Response(null, { status: 204 });
    return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
  });
  vi.stubGlobal("fetch", fetch);
}

/** Renders the page, and sends the topology once the page has subscribed to it. */
async function renderAt(path: string, topology: Topology = house()) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Living room")]);
  await waitFor(() =>
    expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["topology"]) }),
  );
  ws.receive({ type: "topology_snapshot", topic: "topology", topology });
  return { ws, history };
}

describe("the topology page", () => {
  beforeEach(stubApi);

  it("maps the network and details what is selected", async () => {
    const user = userEvent.setup();
    await renderAt("/topology");
    expect(
      await screen.findByRole("img", { name: "Network map: 2 devices and 3 clients" }),
    ).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Devices and clients on the map" });
    await user.click(within(list).getByRole("button", { name: "Living room, Access point" }));
    const details = screen.getByRole("region", { name: "Details" });
    expect(within(details).getByText("Degraded")).toBeInTheDocument();
    expect(within(details).getByText("Gateway over Mesh 5 GHz")).toBeInTheDocument();
    expect(within(details).getByRole("link", { name: "Open device" })).toHaveAttribute(
      "href",
      "/devices/2",
    );

    // The tree groups the access point's two clients; picking one shows it.
    await user.click(within(list).getByRole("button", { name: "2 clients, Client group" }));
    await user.click(
      within(screen.getByRole("region", { name: "Details" })).getByRole("button", {
        name: "phone",
      }),
    );
    const client = screen.getByRole("region", { name: "Details" });
    expect(within(client).getByRole("heading", { name: "phone" })).toBeInTheDocument();
    expect(within(client).getByText("Living room")).toBeInTheDocument();
    expect(within(client).getByText("-55 dBm")).toBeInTheDocument();

    await user.click(within(client).getByRole("button", { name: "Close details" }));
    expect(screen.queryByRole("region", { name: "Details" })).not.toBeInTheDocument();
  });

  it("switches views in the URL and unpins a pinned device", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/topology");
    await user.click(await screen.findByRole("radio", { name: "RF / mesh" }));
    await waitFor(() => expect(history.get()).toBe("/topology?view=rf"));
    expect(screen.getByRole("radio", { name: "RF / mesh" })).toBeChecked();
    const list = screen.getByRole("list", { name: "Devices and clients on the map" });
    expect(within(list).queryByRole("button", { name: /phone/ })).not.toBeInTheDocument();
    await user.click(within(list).getByRole("button", { name: "Living room, Access point" }));
    const details = screen.getByRole("region", { name: "Details" });
    await waitFor(() => expect(within(details).getByText("Pinned")).toBeInTheDocument());
    await user.click(within(details).getByRole("button", { name: "Unpin" }));
    expect(calls).toContain("DELETE /api/v1/topology/layout/node%3A2");
    expect(within(details).queryByText("Pinned")).not.toBeInTheDocument();
  });

  it("replays the topology at a time, then goes live again", async () => {
    const user = userEvent.setup();
    await renderAt("/topology");
    const slider = await screen.findByRole("slider", { name: "Replay the last 24 hours" });
    fireEvent.input(slider, { target: { value: "700" } });
    await waitFor(() => expect(calls).toContain("GET /api/v1/topology/at/700"));
    expect(await screen.findByText("Replaying")).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.getByRole("img", { name: "Network map: 2 devices and 0 clients" }),
      ).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "Back to live" }));
    expect(screen.queryByText("Replaying")).not.toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Network map: 2 devices and 3 clients" }),
    ).toBeInTheDocument();
  });

  it("guides to adoption while there is nothing to map", async () => {
    await renderAt("/topology", { at: 1, nodes: house().nodes.slice(0, 1), edges: [] });
    expect(await screen.findByText("Nothing to map yet")).toBeInTheDocument();
  });
});

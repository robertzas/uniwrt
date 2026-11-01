import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Client } from "../../api/gen/Client";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { createAppRouter } from "../../app/routes";
import { client, detail, NOW, session } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const MAC = "f0:18:98:00:00:01";
let calls: { key: string; body?: unknown }[] = [];

function stubApi() {
  calls = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const key = `${init?.method ?? "GET"} ${url}`;
      calls.push({ key, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (key === `PATCH /api/v1/clients/${encodeURIComponent(MAC)}`)
        return Response.json(client(MAC, { alias: "Anna's phone" }));
      if (key.startsWith(`GET /api/v1/clients/${encodeURIComponent(MAC)}/sessions`))
        return Response.json([
          session(NOW - 5 * 86_400_000, { end: NOW - 4 * 86_400_000, ended_by: "left" }),
        ]);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
}

/** Renders `path`, and once the page has subscribed to `topic`, answers with `message`. */
async function renderAt(path: string, topic: string, message: Record<string, unknown>) {
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
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining([topic]) }));
  ws.receive({ topic, ...message } as ServerMessage);
  return { ws, history };
}

function clients(): Client[] {
  return [
    client(MAC),
    client("5a:00:00:00:00:01", {
      hostname: "Toms-Pixel",
      vendor: null,
      randomized: true,
      group: "name:toms-pixel",
      online: false,
      last_seen: NOW - 600_000,
    }),
    client("5a:00:00:00:00:02", {
      hostname: "Toms-Pixel",
      vendor: null,
      randomized: true,
      group: "name:toms-pixel",
    }),
    client("b8:27:eb:00:00:01", {
      hostname: "raspberrypi",
      vendor: "Raspberry Pi",
      kind: "server",
      online: false,
      experience: null,
      connection: {
        ...(client(MAC).connection as NonNullable<Client["connection"]>),
        node: 1,
        medium: "wired",
        port: "lan2",
        band: null,
        ssid: null,
        signal_dbm: null,
      },
    }),
  ];
}

describe("the clients page", () => {
  beforeEach(stubApi);

  it("lists every client, groups a device's private addresses, and filters", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/clients", "clients", {
      type: "clients_snapshot",
      clients: clients(),
    });
    const table = await screen.findByRole("table", { name: "Clients" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows.map((row) => within(row).getAllByRole("cell")[0]?.textContent)).toEqual([
      "Annas-iPhoneApple · Phone",
      "Toms-PixelPrivate address+1 addressPhone",
      "raspberrypiRaspberry Pi · Server",
    ]);
    expect(within(rows[0] as HTMLElement).getByText("Living room · Home · 5 GHz")).toBeVisible();
    expect(within(rows[2] as HTMLElement).getByText("Gateway · lan2")).toBeVisible();
    expect(screen.getByText("Experience by access point")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Offline" }));
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "All" }));
    await user.type(screen.getByRole("searchbox", { name: "Search clients" }), "pixel");
    expect(within(table).getAllByRole("row")).toHaveLength(2);
    await user.clear(screen.getByRole("searchbox", { name: "Search clients" }));
    await user.type(screen.getByRole("searchbox", { name: "Search clients" }), "nothing");
    expect(within(table).getByText("No clients match.")).toBeVisible();

    await user.clear(screen.getByRole("searchbox", { name: "Search clients" }));
    await user.click(within(table).getByText("Annas-iPhone"));
    await waitFor(() => expect(history.get()).toBe(`/clients/${MAC}`));
  });
});

describe("a client's page", () => {
  beforeEach(stubApi);

  it("shows the connection and score, the timeline and the signal", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    expect(await screen.findByRole("heading", { name: "Annas-iPhone" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Kick" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Block" })).toBeDisabled();
    expect(screen.getByRole("link", { name: "Living room" })).toHaveAttribute("href", "/devices/2");
    expect(screen.getByText("Signal to noise")).toBeVisible();
    for (const cap of ["802.11k", "802.11r", "802.11v"])
      expect(screen.getByText(cap)).toBeVisible();

    await user.click(screen.getByRole("tab", { name: "Timeline" }));
    const lanes = await screen.findByRole("table", { name: "Where it connected" });
    expect(within(lanes).getByText("Roamed")).toBeVisible();
    expect(within(lanes).getByText("Ongoing")).toBeVisible();
    expect(screen.getAllByText("Gateway · Home · 2.4 GHz")).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "7 days" }));
    // The table renders again with the week's sessions.
    expect(await screen.findByText("Left")).toBeVisible();
    expect(calls.some(({ key }) => key.includes("/sessions?since="))).toBe(true);

    await user.click(screen.getByRole("tab", { name: "Signal" }));
    expect(await screen.findByRole("img", { name: "Signal" })).toBeVisible();
    expect(screen.getByRole("img", { name: "Rates" })).toBeVisible();
  });

  it("renames the client", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    await user.click(await screen.findByRole("button", { name: "Rename" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename client" });
    await user.type(within(dialog).getByLabelText("Name"), "  Anna's phone ");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        key: `PATCH /api/v1/clients/${encodeURIComponent(MAC)}`,
        body: { alias: "Anna's phone" },
      }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("says so when the client is unknown", async () => {
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "error",
      message: "There is no client",
    });
    expect(await screen.findByText("Client not found")).toBeVisible();
  });
});

import { toaster } from "@kobalte/core/toast";
import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Client } from "../../api/gen/Client";
import type { Diagnosis } from "../../api/gen/Diagnosis";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import { createAppRouter } from "../../app/routes";
import { client, detail, NOW, session, steered } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { metricsAnswer } from "../../test/metrics";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { Toaster } from "../../ui/Toast";
import { kickToast } from "./ClientActions";

const MAC = "f0:18:98:00:00:01";
let calls: { key: string; body?: unknown }[] = [];
/** What `GET …/doctor` answers: by default, nothing wrong. */
let diagnosis: Diagnosis;

function healthy(): Diagnosis {
  return {
    from: NOW - 86_400_000,
    to: NOW,
    issues: [],
    timeline: [],
    truncated: false,
    dns_logged: true,
  };
}

/** How `POST …/wake` answers: sent, or refused as the gateway in observe mode is. */
let wakeRefused = false;

function stubApi() {
  calls = [];
  wakeRefused = false;
  diagnosis = healthy();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const key = `${init?.method ?? "GET"} ${url}`;
      calls.push({ key, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      if (key === `PATCH /api/v1/clients/${encodeURIComponent(MAC)}`)
        return Response.json(client(MAC, { alias: "Anna's phone" }));
      if (key === `POST /api/v1/clients/${encodeURIComponent(MAC)}/kick`)
        return Response.json({ nodes: 2, from: [{ node: 2, ifname: "phy1-ap0" }], failed: [] });
      if (key === `POST /api/v1/clients/${encodeURIComponent(MAC)}/wake`)
        return wakeRefused
          ? new Response(
              JSON.stringify({
                title: "Conflict",
                status: 409,
                detail: "Gateway is in observe mode and has no etherwake",
              }),
              { status: 409, headers: { "content-type": "application/problem+json" } },
            )
          : Response.json({ node: 1, devices: ["br-lan"], installed: true });
      if (key === `PUT /api/v1/clients/${encodeURIComponent(MAC)}/block`)
        return Response.json(client(MAC, { blocked: NOW }));
      if (key === `DELETE /api/v1/clients/${encodeURIComponent(MAC)}/block`)
        return Response.json(client(MAC));
      if (key.startsWith(`GET /api/v1/clients/${encodeURIComponent(MAC)}/doctor?`))
        return Response.json(diagnosis);
      if (key.startsWith(`GET /api/v1/clients/${encodeURIComponent(MAC)}/sessions`))
        return Response.json([
          session(NOW - 5 * 86_400_000, { end: NOW - 4 * 86_400_000, ended_by: "left" }),
        ]);
      if (key.startsWith("GET /api/v1/metrics?")) return Response.json(metricsAnswer(url));
      if (key.startsWith("GET /api/v1/events?"))
        return Response.json({
          events: [
            steered(Date.now() - 60_000),
            steered(
              Date.now() - 120_000,
              {
                action: "assoc_denied",
                reason: "low_signal",
                signal_dbm: -80,
                min_signal_dbm: -75,
                to: null,
              },
              { id: 2, count: 3, level: "notice" },
            ),
          ],
          next: null,
        });
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
        <Toaster />
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
  beforeEach(() => {
    stubApi();
    // Toasts outlive a test's render: the toaster is global.
    toaster.clear();
  });

  it("shows the connection and score, the timeline and the signal", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    expect(await screen.findByRole("heading", { name: "Annas-iPhone" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Kick" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Block" })).toBeEnabled();
    expect(screen.getByRole("link", { name: "Living room" })).toHaveAttribute("href", "/devices/2");
    expect(screen.getByText("Signal to noise")).toBeVisible();
    // What it showed over its associations: the standard, the bands, the features.
    expect(screen.getByText("Wi-Fi generation").nextElementSibling).toHaveTextContent("Wi-Fi 6");
    expect(screen.getByText("Bands").nextElementSibling).toHaveTextContent("2.4 GHz · 5 GHz");
    for (const cap of ["802.11k", "802.11r", "802.11v"])
      expect(screen.getByText(cap)).toBeVisible();

    await user.click(screen.getByRole("tab", { name: "Timeline" }));
    const lanes = await screen.findByRole("table", { name: "Where it connected" });
    expect(within(lanes).getByText("Roamed")).toBeVisible();
    expect(within(lanes).getByText("Ongoing")).toBeVisible();
    expect(screen.getAllByText("Gateway · Home · 2.4 GHz")).toHaveLength(2);
    // Steering: marks on the access point's lane, and in words under the lanes.
    const steering = await screen.findByRole("region", { name: "Steering" });
    expect(steering).toHaveTextContent("1 moved · 3 turned away");
    expect(steering).toHaveTextContent("Annas-iPhone agreed to move from Living room to Gateway");
    expect(steering).toHaveTextContent(
      "Living room turned Annas-iPhone away · Signal -80 dBm, under -75 dBm",
    );
    expect(screen.getAllByRole("img", { name: /agreed to move from Living room/ })).toHaveLength(1);
    const read = calls.find(({ key }) => key.startsWith("GET /api/v1/events?"))?.key ?? "";
    const params = new URL(read.slice(4), "http://test").searchParams;
    expect(params.get("kinds")).toBe("client_steered");
    expect(params.get("clients")).toBe(MAC);
    await user.click(screen.getByRole("button", { name: "7 days" }));
    // The table renders again with the week's sessions.
    expect(await screen.findByText("Left")).toBeVisible();
    expect(calls.some(({ key }) => key.includes("/sessions?since="))).toBe(true);

    await user.click(screen.getByRole("tab", { name: "Signal" }));
    expect(await screen.findByRole("img", { name: "Signal" })).toBeVisible();
    expect(screen.getByRole("img", { name: "Rates" })).toBeVisible();
  });

  it("charts signal and traffic, live for the last hour and stored beyond", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    const reads = () =>
      calls
        .filter(({ key }) => key.startsWith("GET /api/v1/metrics?"))
        .map(({ key }) => new URL(key.slice(4), "http://test").searchParams);
    await user.click(await screen.findByRole("tab", { name: "Signal" }));
    expect(await screen.findByText("Live samples")).toBeVisible();
    expect(screen.getByText("-58 dBm")).toBeVisible();
    expect(reads()).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Last 24 hours" }));
    await waitFor(() => expect(reads()).toHaveLength(1));
    expect(reads()[0]?.get("entity")).toBe(`client:${MAC}`);
    expect(reads()[0]?.get("metrics")).toBe("signal,tx_rate,rx_rate");
    expect(await screen.findByText("15-second averages")).toBeVisible();
    expect(await screen.findByText("42 dBm")).toBeVisible();
    expect(screen.getByRole("link", { name: "Open in Metrics" })).toHaveAttribute(
      "href",
      `/insights/metrics?entity=client%3A${encodeURIComponent(MAC)}&range=24h`,
    );

    await user.click(screen.getByRole("tab", { name: "Traffic" }));
    expect(await screen.findByRole("img", { name: "Throughput" })).toBeVisible();
    // 2 Mb/s down and 100 kb/s up over the 5 s between the two samples.
    expect(screen.getByText("1.3 MB")).toBeVisible();
    expect(screen.getByText("62.5 KB")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Last 7 days" }));
    await waitFor(() => expect(reads().at(-1)?.get("metrics")).toBe("down_bps,up_bps"));
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

  it("kicks the client for a minute unless told otherwise, and blocks it after asking", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    await user.click(await screen.findByRole("button", { name: "Kick" }));
    const kick = await screen.findByRole("dialog", { name: "Kick this client" });
    expect(within(kick).getByRole("button", { name: "1 min" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(within(kick).getByRole("button", { name: "10 min" }));
    await user.click(within(kick).getByRole("button", { name: "Kick" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        key: `POST /api/v1/clients/${encodeURIComponent(MAC)}/kick`,
        body: { ban_secs: 600 },
      }),
    );
    expect(await screen.findByText("Kicked from Living room")).toBeVisible();
    expect(screen.getByText("It's kept off for 10 min.")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Block" }));
    const block = await screen.findByRole("dialog", { name: "Block Annas-iPhone?" });
    await user.click(within(block).getByRole("button", { name: "Block" }));
    await waitFor(() =>
      expect(calls.map(({ key }) => key)).toContain(
        `PUT /api/v1/clients/${encodeURIComponent(MAC)}/block`,
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("wakes the client through the gateway, and keeps a refusal in the dialog", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    wakeRefused = true;
    await user.click(await screen.findByRole("button", { name: "Wake" }));
    const dialog = await screen.findByRole("dialog", { name: "Wake Annas-iPhone?" });
    await user.click(within(dialog).getByRole("button", { name: "Wake" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Gateway is in observe mode and has no etherwake",
    );

    wakeRefused = false;
    await user.click(within(dialog).getByRole("button", { name: "Wake" }));
    expect(await screen.findByText("Wake-on-LAN sent to Annas-iPhone")).toBeVisible();
    expect(
      screen.getByText("Sent on br-lan. The gateway installed etherwake for it."),
    ).toBeVisible();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows a blocked client as such and unblocks it", async () => {
    const user = userEvent.setup();
    const blocked = detail();
    blocked.client = client(MAC, { blocked: NOW, online: false });
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: blocked,
    });
    const heading = await screen.findByRole("heading", { name: "Annas-iPhone" });
    expect(within(heading.parentElement as HTMLElement).getByText("Blocked")).toBeVisible();
    expect(screen.getByRole("button", { name: "Kick" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Unblock" }));
    await waitFor(() =>
      expect(calls.map(({ key }) => key)).toContain(
        `DELETE /api/v1/clients/${encodeURIComponent(MAC)}/block`,
      ),
    );
    expect(await screen.findByText("Unblocked: it can connect again")).toBeVisible();
  });

  it("says so when the client is unknown", async () => {
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "error",
      message: "There is no client",
    });
    expect(await screen.findByText("Client not found")).toBeVisible();
  });
});

describe("a kick's toast", () => {
  const name = (id: number) => (id === 2 ? "Living room" : `#${id}`);
  it("says when no device manages Wi-Fi, which refused, and where it was kicked from", () => {
    expect(kickToast({ nodes: 0, from: [], failed: [] }, "60", name)).toMatchObject({
      title: "No device manages Wi-Fi",
      tone: "warn",
    });
    expect(
      kickToast({ nodes: 2, from: [], failed: [{ node: 2, error: "refused" }] }, "60", name),
    ).toEqual({ title: "Not kicked on Living room", description: "refused", tone: "crit" });
    expect(kickToast({ nodes: 1, from: [], failed: [] }, "0", name)).toEqual({
      title: "It wasn't connected to a managed access point",
      description: undefined,
      tone: "ok",
    });
  });
});

describe("a client's doctor", () => {
  beforeEach(() => {
    stubApi();
    toaster.clear();
  });

  it("shows a chip on the Overview, then the issues with their fixes and the timeline", async () => {
    const user = userEvent.setup();
    diagnosis = {
      ...healthy(),
      issues: [
        {
          severity: "warning",
          problem: { kind: "weak_signal", signal_dbm: -78, node: 2, band: "5g" },
          at: NOW - 60_000,
          fixes: [
            { fix: "move_closer", node: 2 },
            { fix: "turn_on_steering", ssid: "Home" },
            { fix: "reconnect" },
          ],
        },
        {
          severity: "info",
          problem: { kind: "dns_blocked", blocked: 30, queries: 100 },
          at: NOW - 120_000,
          fixes: [{ fix: "bypass_filtering" }],
        },
      ],
      timeline: [
        { kind: "dns", at: NOW - 60_000, failed: 4, domains: ["a.example", "b.example"] },
        {
          kind: "wifi",
          at: NOW - 120_000,
          node: 2,
          step: "auth_failed",
          interface: "phy1-ap0",
          detail: "4-way handshake timed out",
        },
        {
          kind: "dhcp",
          at: NOW - 180_000,
          node: 1,
          step: "ack",
          interface: "br-lan",
          ip: "192.168.1.20",
          detail: null,
        },
        { kind: "event", at: NOW - 240_000, event: steered(NOW - 240_000) },
      ],
      dns_logged: false,
    };
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    const chip = await screen.findByRole("button", { name: /Weak signal and 1 more/ });
    await user.click(chip);
    expect(screen.getByRole("tab", { name: "Doctor" })).toHaveAttribute("aria-selected", "true");

    const issues = await screen.findByRole("region", { name: "Issues" });
    const [weak, blocked] = within(issues)
      .getAllByRole("listitem")
      .filter((li) => li.querySelector("h3"));
    expect(weak).toHaveTextContent("Warning");
    expect(weak).toHaveTextContent(
      "-78 dBm by the median of the last 15 minutes, at Living room · 5 GHz",
    );
    expect(weak).toHaveTextContent("Move it closer to Living room");
    expect(within(weak as HTMLElement).getByRole("link", { name: "Wi-Fi" })).toHaveAttribute(
      "href",
      "/wifi",
    );
    await user.click(
      within(weak as HTMLElement).getByRole("button", { name: "Kick it to reconnect" }),
    );
    expect(await screen.findByRole("dialog", { name: "Kick this client" })).toBeVisible();
    await user.keyboard("{Escape}");
    expect(blocked).toHaveTextContent("30 of 100 lookups were blocked by DNS filtering (30 %)");
    expect(within(blocked as HTMLElement).getByRole("link", { name: "Networks" })).toHaveAttribute(
      "href",
      "/networks",
    );
    expect(issues).toHaveTextContent("DNS isn't checked");

    const timeline = screen.getByRole("region", { name: "Timeline" });
    const rows = within(timeline).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector("p")?.textContent)).toEqual([
      "4 failed DNS lookups",
      "Handshake failed",
      "Got an address",
      "Annas-iPhone agreed to move from Living room to Gateway",
    ]);
    expect(rows[1]).toHaveTextContent("phy1-ap0 · 4-way handshake timed out · Living room");
    expect(rows[2]).toHaveTextContent("192.168.1.20 · br-lan · Gateway");

    await user.click(screen.getByRole("button", { name: "7 days" }));
    await waitFor(() =>
      expect(calls.some(({ key }) => key.endsWith("/doctor?hours=168"))).toBe(true),
    );
  });

  it("says when nothing is wrong, and shows no chip", async () => {
    const user = userEvent.setup();
    await renderAt(`/clients/${MAC}`, `client:${MAC}`, {
      type: "client_snapshot",
      client: detail(),
    });
    expect(await screen.findByRole("heading", { name: "Annas-iPhone" })).toBeVisible();
    await waitFor(() =>
      expect(calls.some(({ key }) => key.includes("/doctor?hours=24"))).toBe(true),
    );
    expect(screen.queryByRole("button", { name: /Weak signal/ })).toBeNull();
    await user.click(screen.getByRole("tab", { name: "Doctor" }));
    expect(await screen.findByText("No issues found")).toBeVisible();
    expect(screen.getByText("Nothing happened in this range.")).toBeVisible();
  });
});

import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LogRecord } from "../../api/gen/LogRecord";
import type { SavedSearch } from "../../api/gen/SavedSearch";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import type { FakeWebSocket } from "../../test/socket";

const NOW = Date.now();
let stored: LogRecord[] = [];
let saved: SavedSearch[] = [];
let calls: { url: string; method: string; body?: unknown }[] = [];

function entry(id: number, extra: Partial<LogRecord> = {}): LogRecord {
  return {
    id,
    received: NOW - 60_000 + id * 1_000,
    node: 1,
    host: null,
    via: "agent",
    pattern: null,
    seq: id,
    time: NOW - 60_000 + id * 1_000,
    facility: 3,
    severity: "info",
    program: "netifd",
    pid: 812,
    message: `entry ${id}`,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  saved = [];
  stored = [
    entry(3, {
      program: "hostapd",
      severity: "notice",
      pattern: "hostapd-inactivity",
      message: "wlan0: STA 02:00:00:00:04:00 IEEE 802.11: disassociated due to inactivity",
    }),
    entry(2, {
      node: null,
      host: "switch",
      program: null,
      severity: "error",
      message: "port 3 down",
    }),
    entry(1),
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      calls.push({ url, method, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      const path = url.split("?")[0];
      if (path === "/api/v1/logs") return Response.json({ records: stored, next: null });
      if (path === "/api/v1/logs/histogram") {
        const from = NOW - 3_600_000;
        return Response.json({
          from,
          to: NOW,
          step_ms: 60_000,
          buckets: Array.from({ length: 60 }, (_, i) => ({
            start: from + i * 60_000,
            counts: i === 59 ? [0, 0, 0, 1, 0, 1, 1, 0] : [0, 0, 0, 0, 0, 0, 0, 0],
          })),
        });
      }
      if (path === "/api/v1/logs/patterns") {
        return Response.json({
          patterns: [
            {
              id: "hostapd-inactivity",
              programs: ["hostapd"],
              severity: "info",
              meaning: "The client went quiet and the access point let it go",
              hint: "Usually a phone asleep or out of range.",
              link: null,
            },
          ],
        });
      }
      if (path === "/api/v1/logs/sources") {
        return Response.json({ sources: { hosts: ["switch"], programs: ["hostapd", "netifd"] } });
      }
      if (path === "/api/v1/logs/searches" && method === "POST") {
        const body = JSON.parse(String(init?.body)) as Pick<SavedSearch, "name" | "filter">;
        const search = { id: saved.length + 1, ...body, created_by: 1, created_at: NOW };
        saved.push(search);
        return Response.json(search, { status: 201 });
      }
      if (path === "/api/v1/logs/searches") return Response.json({ searches: saved });
      if (path?.startsWith("/api/v1/logs/") && path.endsWith("/context")) {
        return Response.json({
          records: [entry(0, { node: 2, message: "before it" }), ...stored.slice().reverse()],
        });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderAt(path: string, topic = "logs") {
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
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining([topic]) }));
  return { ws, history };
}

const entries = () =>
  within(screen.getByRole("list", { name: "Log entries" }))
    .getAllByRole("listitem")
    .map((item) => item.textContent ?? "");

function deliver(ws: FakeWebSocket, records: LogRecord[], topic = "logs") {
  ws.receive({ type: "log_delta", topic, entries: records, dropped: 0 });
}

describe("the logs page", () => {
  it("lists the stored entries, then shows new ones as they arrive", async () => {
    const { ws } = await renderAt("/insights/logs");
    expect(await screen.findByText("port 3 down")).toBeVisible();
    expect(entries()).toHaveLength(3);
    expect(entries()[1]).toContain("switch");
    const page = calls.find((c) => c.url.startsWith("/api/v1/logs?"))?.url ?? "";
    const params = new URL(page, "http://test").searchParams;
    expect(params.get("limit")).toBe("200");
    expect(Date.now() - Number(params.get("from"))).toBeGreaterThanOrEqual(3_600_000);
    expect(screen.getByText("Live")).toBeVisible();

    // An entry stored after the page was read; one the page already shows comes once.
    deliver(ws, [entry(4, { message: "fresh one" }), stored[0] as LogRecord]);
    await waitFor(() => expect(entries()[0]).toContain("fresh one"));
    expect(entries()).toHaveLength(4);
    // The histogram counts the range by severity.
    expect(
      screen.getByRole("img", { name: /Log volume: 3 entries, 1 errors and 0 warnings/ }),
    ).toBeVisible();
  });

  it("holds arrivals while paused and shows them on demand", async () => {
    const user = userEvent.setup();
    const { ws } = await renderAt("/insights/logs");
    await screen.findByText("port 3 down");
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(screen.getByText("Paused")).toBeVisible();
    deliver(ws, [entry(4, { message: "held one" }), entry(5, { message: "held two" })]);
    const show = await screen.findByRole("button", { name: "Show 2 new entries" });
    expect(screen.queryByText("held one")).toBeNull();
    await user.click(show);
    expect(await screen.findByText("held two")).toBeVisible();
    expect(entries()).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Pause" })).toBeVisible();
  });

  it("filters through the URL, the search box and the hub's topic", async () => {
    const user = userEvent.setup();
    const { ws, history } = await renderAt(
      "/insights/logs?nodes=1&min=warning",
      "logs?node=1&min=warning",
    );
    await screen.findByText("port 3 down");
    // What the hub cannot narrow, the page does: a live entry of another program is left out.
    await user.type(screen.getByRole("searchbox", { name: "Search the logs" }), "port{Enter}");
    await waitFor(() =>
      expect(new URL(history.get(), "http://t").searchParams.get("q")).toBe("port"),
    );
    await waitFor(() =>
      expect(calls.some((c) => c.url.startsWith("/api/v1/logs?") && c.url.includes("q=port"))).toBe(
        true,
      ),
    );
    const topic = "logs?node=1&min=warning";
    deliver(ws, [entry(7, { severity: "error", message: "port 1 up" })], topic);
    deliver(ws, [entry(8, { severity: "error", message: "radio up" })], topic);
    expect(await screen.findByRole("button", { name: /port 1 up/ })).toBeVisible();
    expect(screen.queryByText("radio up")).toBeNull();
    // The search's words are marked.
    expect(screen.getAllByText("port", { selector: "mark" }).length).toBeGreaterThan(0);

    await user.selectOptions(screen.getByRole("combobox", { name: "Severity" }), "error");
    await waitFor(() => expect(history.get()).toContain("min=error"));
    await user.click(screen.getByRole("button", { name: "Explained only" }));
    await waitFor(() => expect(history.get()).toContain("annotated=1"));
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(history.get()).toBe("/insights/logs"));
  });

  it("explains an annotated entry and opens the context around it", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/logs");
    const line = await screen.findByRole("button", { name: /disassociated due to inactivity/ });
    await user.click(line);
    expect(line).toHaveAttribute("aria-expanded", "true");
    const note = await screen.findByRole("complementary", { name: "What it means" });
    expect(note).toHaveTextContent("The client went quiet and the access point let it go");
    expect(note).toHaveTextContent("Usually a phone asleep");
    expect(screen.getByRole("link", { name: "Gateway" })).toHaveAttribute("href", "/devices/1");

    await user.click(screen.getByRole("button", { name: "Show context" }));
    const dialog = await screen.findByRole("dialog", { name: "Context" });
    const around = await within(dialog).findByRole("list", { name: "Entries around it" });
    const items = within(around).getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("before it");
    expect(items.find((item) => item.getAttribute("aria-current") === "true")).toHaveTextContent(
      "inactivity",
    );
    expect(calls.some((c) => c.url === "/api/v1/logs/3/context?around=20")).toBe(true);
    await user.click(within(dialog).getByRole("button", { name: "Show more" }));
    await waitFor(() =>
      expect(calls.some((c) => c.url === "/api/v1/logs/3/context?around=50")).toBe(true),
    );
  });

  it("narrows to one source from an entry, and saves the search", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/insights/logs");
    await user.click(await screen.findByRole("button", { name: /port 3 down/ }));
    await user.click(screen.getByRole("button", { name: "Only this source" }));
    await waitFor(() => expect(history.get()).toContain("hosts=switch"));

    await user.click(screen.getByRole("button", { name: "Saved searches" }));
    await user.click(await screen.findByRole("button", { name: "Save this search…" }));
    const dialog = await screen.findByRole("dialog", { name: "Save search" });
    await user.type(within(dialog).getByLabelText("Name"), "Switch");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      name: "Switch",
      filter: {
        nodes: [],
        hosts: ["switch"],
        programs: [],
        min: null,
        text: null,
        annotated: false,
        from: null,
        to: null,
      },
    });
    // The search in use names the button.
    expect(await screen.findByRole("button", { name: "Saved searches: Switch" })).toBeVisible();
  });

  it("says what it found nothing for", async () => {
    stored = [];
    const user = userEvent.setup();
    await renderAt("/insights/logs?programs=dropbear");
    expect(await screen.findByText("Nothing matches these filters in this range.")).toBeVisible();
    await user.click(screen.getAllByRole("button", { name: "Clear filters" })[0] as HTMLElement);
    expect(
      await screen.findByText("Entries appear here as soon as the nodes log them."),
    ).toBeVisible();
  });
});

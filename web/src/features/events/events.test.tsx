import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EventRecord } from "../../api/gen/EventRecord";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const NOW = Date.now();
let stored: EventRecord[] = [];
let calls: string[] = [];

function event(
  id: number,
  what: EventRecord["what"],
  extra: Partial<EventRecord> = {},
): EventRecord {
  return {
    id,
    at: NOW - 60_000 + id * 1_000,
    last_at: NOW - 60_000 + id * 1_000,
    count: 1,
    level: "notice",
    category: "system",
    node: 2,
    client: null,
    condition: null,
    what,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  stored = [
    event(
      3,
      { kind: "wan_down", device: "eth1" },
      {
        node: 1,
        level: "critical",
        category: "wan",
        condition: { key: "node:1/wan:eth1", raised: true },
      },
    ),
    event(
      2,
      {
        kind: "log_pattern",
        pattern: "dfs-radar",
        log: 7,
        host: null,
        program: "hostapd",
        text: "DFS-RADAR-DETECTED freq=5500",
      },
      { category: "wifi", level: "warning", count: 3 },
    ),
    event(1, { kind: "node_adopted" }),
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      calls.push(url);
      const path = url.split("?")[0];
      if (path === "/api/v1/events") return Response.json({ events: stored, next: null });
      if (path === "/api/v1/logs/patterns") {
        return Response.json({
          patterns: [
            {
              id: "dfs-radar",
              programs: ["hostapd"],
              severity: "warning",
              meaning: "Radar was detected on a DFS channel",
              hint: null,
              link: null,
              event: "wifi",
            },
          ],
        });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderAt(path: string, topic = "events") {
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

const rows = () =>
  within(screen.getByRole("region", { name: "Events" }))
    .getAllByRole("listitem")
    .map((item) => item.textContent ?? "");

describe("the events page", () => {
  it("lists the stored events by day, then new and folded ones as they arrive", async () => {
    const { ws } = await renderAt("/insights/events");
    expect(await screen.findByText("Internet down on Gateway (eth1)")).toBeVisible();
    expect(screen.getByText("Today")).toBeVisible();
    expect(rows()).toHaveLength(3);
    expect(rows()[0]).toContain("Ongoing");
    expect(rows()[1]).toContain("Attic AP: Radar was detected on a DFS channel");
    expect(rows()[1]).toContain("×3");
    const page = new URL(calls.find((c) => c.startsWith("/api/v1/events?")) ?? "", "http://x");
    expect(page.searchParams.get("limit")).toBe("100");
    // 24 h before the page asked: after this file loaded (NOW), before this check.
    const from = Number(page.searchParams.get("from"));
    expect(from).toBeGreaterThanOrEqual(NOW - 86_400_000);
    expect(from).toBeLessThanOrEqual(Date.now() - 86_400_000);

    // The WAN comes back; the radar repeats (a fold: the same id, counted).
    const up = event(
      4,
      { kind: "wan_up", device: "eth1", down_ms: 90_000 },
      { node: 1, category: "wan", condition: { key: "node:1/wan:eth1", raised: false } },
    );
    const folded = { ...(stored[1] as EventRecord), count: 5 };
    ws.receive({ type: "events_delta", topic: "events", events: [folded, up] });
    await waitFor(() => expect(rows()[0]).toContain("Internet back on Gateway (eth1)"));
    expect(rows()).toHaveLength(4);
    expect(rows()[0]).toContain("Down for 1m 30s");
    expect(rows()[1]).not.toContain("Ongoing");
    expect(rows()[2]).toContain("×5");
  });

  it("filters through the URL and the hub's topic, and links to the logs around an event", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt(
      "/insights/events?nodes=1&min=warning",
      "events?node=1&min=warning",
    );
    await screen.findByText("Internet down on Gateway (eth1)");
    const page = new URL(calls.find((c) => c.startsWith("/api/v1/events?")) ?? "", "http://x");
    expect(page.searchParams.get("nodes")).toBe("1");
    expect(page.searchParams.get("min")).toBe("warning");

    await user.click(screen.getByRole("button", { name: /Internet down on Gateway/ }));
    const logs = screen.getByRole("link", { name: "Logs around this" });
    const href = new URL(logs.getAttribute("href") ?? "", "http://x");
    expect(href.pathname).toBe("/insights/logs");
    expect(href.searchParams.get("nodes")).toBe("1");
    expect(screen.getByRole("link", { name: "Open device" })).toHaveAttribute("href", "/devices/1");

    await user.click(screen.getByRole("button", { name: "Internet" }));
    await waitFor(() => expect(history.get()).toContain("categories=wan"));
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await waitFor(() => expect(history.get()).toBe("/insights/events"));
  });

  it("says so when nothing happened", async () => {
    stored = [];
    await renderAt("/insights/events?range=1h");
    expect(await screen.findByText("No events")).toBeVisible();
  });
});

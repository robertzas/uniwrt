import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EventRecord } from "../../../api/gen/EventRecord";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts } from "../../../test/fixtures";
import { NO_PACKAGES } from "../../../test/packages";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig = siteConfig();
let events: EventRecord[] = [];

function event(id: number, what: EventRecord["what"]): EventRecord {
  const at = Date.now() - id * 3600_000;
  return {
    id,
    at,
    last_at: at,
    count: 1,
    level: "warning",
    category: "system",
    node: 1,
    client: null,
    condition: null,
    what,
  };
}

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  config.schedules = {
    reboot: { days: ["sun"], at: 240 },
    leds_off: {
      spans: [{ days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], from: 1380, to: 420 }],
    },
  };
  events = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head, config));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        config = (body as SiteSave).config;
        return Response.json(siteView(head, config));
      }
      if (url.startsWith("/api/v1/events?")) return Response.json({ events, next: null });
      if (url === "/api/v1/firmware") return Response.json({ nodes: [], error: null });
      if (url === "/api/v1/nodes/1/packages") return Response.json(NO_PACKAGES);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderSchedules(managed = true) {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=config"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await waitFor(
    () => {
      const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
      expect(sent?.sub).toContain("node:1");
    },
    { timeout: 5000 },
  );
  const node = detail(facts(1));
  node.node.mode = managed ? "managed" : "observe";
  if (node.facts) node.facts.system.leds = { count: 3, lit: 0, held: true };
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  return within(await screen.findByRole("region", { name: "Schedules" }, { timeout: 3000 }));
}

describe("Device › Config › Schedules", () => {
  it("shows the site's schedules as the device follows them, and what happened", async () => {
    events = [
      event(1, { kind: "led_schedule_failed", error: "led1: write failed" }),
      event(2, { kind: "scheduled_reboot_missed", why: "offline" }),
    ];
    const card = await renderSchedules();
    expect(card.getByText("The site's schedule: Sun 04:00.")).toBeInTheDocument();
    expect(card.getByText(/^Next restart: /)).toBeInTheDocument();
    expect(card.getByText(/Every day 23:00–07:00/)).toBeInTheDocument();
    expect(card.getByText(/^Its 3 LEDs are dark\./)).toHaveTextContent(
      "The controller holds them off",
    );
    expect(await card.findByText(/^Missed on .*: It was offline/)).toBeInTheDocument();
    expect(card.getByRole("alert")).toHaveTextContent("led1: write failed");
  });

  it("gives the device its own restart time and leaves it out of the LEDs schedule", async () => {
    const user = userEvent.setup();
    events = [
      event(1, { kind: "led_schedule_ok" }),
      event(2, { kind: "led_schedule_failed", error: "old" }),
    ];
    const card = await renderSchedules();
    expect(card.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(card.getByRole("button", { name: "Its own" }));
    // It starts from the site's time.
    expect(card.getByRole("button", { name: "Sunday" })).toHaveAttribute("aria-pressed", "true");
    await user.click(card.getByRole("button", { name: "Monday" }));
    await user.click(card.getByLabelText("Turn its LEDs off with the site's schedule"));
    await user.click(card.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    const saved = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(saved.config.nodes.find((n) => n.node === 1)).toMatchObject({
      reboot: { kind: "own", days: ["mon", "sun"], at: 240 },
      led_schedule: false,
    });
    expect(saved.message).toBe("Edit gw's schedules");
    expect(await card.findByText(/without an apply/)).toBeInTheDocument();

    await user.click(card.getByRole("button", { name: "Never" }));
    expect(card.getByText("No restart scheduled.")).toBeInTheDocument();
  });

  it("says that a device in observe mode follows no schedule", async () => {
    const card = await renderSchedules(false);
    expect(card.getByText(/^Only managed devices follow schedules/)).toBeInTheDocument();
    expect(card.queryByText(/^Next restart: /)).not.toBeInTheDocument();
  });
});

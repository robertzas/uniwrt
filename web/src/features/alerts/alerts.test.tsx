import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MaintenanceWindow } from "../../api/gen/MaintenanceWindow";
import type { RuleView } from "../../api/gen/RuleView";
import { createAppRouter } from "../../app/routes";
import { alertRecord, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const NOW = Date.now();
let calls: { url: string; method: string; body: unknown }[] = [];
let rules: RuleView[] = [];
let windows: MaintenanceWindow[] = [];

const cpu: RuleView = {
  id: 6,
  preset: "cpu",
  modified: false,
  name: "Device CPU busy",
  enabled: true,
  level: "warning",
  when: {
    type: "metric",
    metric: "cpu",
    entities: "node:*",
    op: "above",
    threshold: 90,
    clear: 80,
    for_s: 300,
  },
  cooldown_s: 900,
};

const mine: RuleView = {
  id: 40,
  preset: null,
  modified: false,
  name: "Radar on the attic",
  enabled: true,
  level: "notice",
  when: {
    type: "event",
    kinds: ["log_pattern"],
    categories: [],
    min: null,
    nodes: [2],
    patterns: ["dfs-radar"],
    count: 1,
    window_s: 900,
  },
  cooldown_s: 0,
};

beforeEach(() => {
  calls = [];
  rules = [cpu, mine];
  windows = [
    {
      id: 1,
      starts_at: NOW - 60_000,
      ends_at: NOW + 3_600_000,
      nodes: [2],
      note: "Rewiring",
      created_by: 1,
    },
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ url, method, body });
      const path = url.split("?")[0] ?? "";
      if (path === "/api/v1/alerts") {
        return Response.json({
          alerts: [
            alertRecord(9, {
              started_at: NOW - 3_600_000,
              resolved_at: NOW - 1_800_000,
              resolution: "cleared",
              name: "Internet down",
              level: "critical",
              subject: "node:1/wan:eth1",
              cause: {
                type: "event",
                event: 3,
                kind: "wan_down",
                matches: 1,
                condition: "node:1/wan:eth1",
              },
            }),
          ],
          next: null,
        });
      }
      if (path === "/api/v1/alert-rules" && method === "GET") return Response.json({ rules });
      if (path === "/api/v1/alert-rules" && method === "POST") {
        return Response.json({ ...body, id: 41, preset: null, modified: false }, { status: 201 });
      }
      if (path.startsWith("/api/v1/alert-rules/") && method === "PUT") {
        return Response.json({ ...cpu, ...body, modified: true });
      }
      if (path === "/api/v1/maintenance" && method === "GET") return Response.json({ windows });
      if (path === "/api/v1/maintenance" && method === "POST") {
        return Response.json({ ...body, id: 2, created_by: 1 }, { status: 201 });
      }
      if (method === "DELETE") return new Response(null, { status: 204 });
      if (path.endsWith("/acknowledge")) {
        return Response.json(
          alertRecord(1, { acknowledged: { by: 1, username: "admin", at: NOW } }),
        );
      }
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

async function renderAt(path: string, role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
  await screen.findByRole("heading", { name: "Alerts", level: 1 });
  return { ws, history };
}

const posted = (path: string) => calls.filter((c) => c.method !== "GET" && c.url === path);

describe("the alerts page", () => {
  it("lists the alerts firing, opens the bell's one and acknowledges", async () => {
    const user = userEvent.setup();
    const { ws } = await renderAt("/insights/alerts?alert=1");
    ws.receive({
      type: "alerts_snapshot",
      topic: "alerts",
      alerts: [
        alertRecord(1),
        alertRecord(2, { silenced: true, node: 2, subject: "node:2", name: "Device running hot" }),
      ],
    });
    const list = await screen.findByRole("region", { name: "Active alerts" });
    await within(list).findByText("2 firing");
    expect(within(list).getByText(/1 needs attention/)).toBeVisible();
    // The bell's alert is open: its details and actions show.
    expect(within(list).getByRole("link", { name: "Open device" })).toHaveAttribute(
      "href",
      "/devices/1",
    );
    expect(within(list).getByRole("link", { name: "Edit the rule" })).toHaveAttribute(
      "href",
      "/insights/alerts?tab=rules&rule=6",
    );
    expect(
      within(list).getByText("CPU 93 %, above 90 % · worst 97 %", { selector: "dd" }),
    ).toBeVisible();

    await user.click(
      within(list).getByRole("button", { name: "Acknowledge: Device CPU busy, Gateway" }),
    );
    await waitFor(() => expect(posted("/api/v1/alerts/1/acknowledge")).toHaveLength(1));
  });

  it("shows the history and filters it through the URL", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/insights/alerts?tab=history&range=30d");
    const region = await screen.findByRole("region", { name: "Alert history" });
    expect(await within(region).findByText("Internet down")).toBeVisible();
    expect(within(region).getByText(/Gateway · eth1/)).toBeVisible();
    expect(within(region).getByText(/Resolved · Cleared/)).toBeVisible();
    const page = new URL(
      calls.find((c) => c.url.startsWith("/api/v1/alerts?"))?.url ?? "",
      "http://x",
    );
    const back = Date.now() - Number(page.searchParams.get("from"));
    expect(back).toBeGreaterThanOrEqual(30 * 86_400_000);
    expect(back).toBeLessThan(30 * 86_400_000 + 60_000);

    await user.selectOptions(within(region).getByLabelText("Level"), "critical");
    await waitFor(() => expect(history.get()).toContain("min=critical"));
    await waitFor(() => expect(calls.some((c) => c.url.includes("min=critical"))).toBe(true));
  });

  it("toggles, edits and adds rules", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/alerts?tab=rules");
    const builtIn = await screen.findByRole("region", { name: "Built in" });
    expect(within(builtIn).getByText(/CPU above 90 % for 5m · Every device/)).toBeVisible();
    const own = screen.getByRole("region", { name: "Your rules" });
    expect(
      await within(own).findByText(/Radar was detected on a DFS channel · on Attic AP/),
    ).toBeVisible();

    await user.click(within(builtIn).getByRole("checkbox", { name: "Turn off “Device CPU busy”" }));
    await waitFor(() => expect(posted("/api/v1/alert-rules/6")).toHaveLength(1));
    expect(posted("/api/v1/alert-rules/6")[0]?.body).toMatchObject({
      enabled: false,
      name: "Device CPU busy",
    });
    expect(await within(builtIn).findByText("Modified")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "New rule" }));
    const dialog = await screen.findByRole("dialog", { name: "New alert rule" });
    await user.click(within(dialog).getByRole("button", { name: "Save rule" }));
    expect(within(dialog).getByText("Give the rule a name (at most 80 characters).")).toBeVisible();
    await user.type(within(dialog).getByLabelText("Name"), "Radios busy");
    await user.selectOptions(within(dialog).getByLabelText("Watch"), "radio");
    await user.clear(within(dialog).getByLabelText("Threshold"));
    await user.type(within(dialog).getByLabelText("Threshold"), "70");
    await user.click(within(dialog).getByRole("button", { name: "Save rule" }));
    await waitFor(() => expect(posted("/api/v1/alert-rules")).toHaveLength(1));
    expect(posted("/api/v1/alert-rules")[0]?.body).toEqual({
      name: "Radios busy",
      enabled: true,
      level: "warning",
      cooldown_s: 900,
      when: {
        type: "metric",
        metric: "busy",
        entities: "node:*/radio:*",
        op: "above",
        threshold: 70,
        clear: null,
        for_s: 300,
      },
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await within(own).findByText("Radios busy")).toBeVisible();
  });

  it("opens a rule's editor from an alert", async () => {
    await renderAt("/insights/alerts?tab=rules&rule=6");
    const dialog = await screen.findByRole("dialog", { name: "Edit “Device CPU busy”" });
    expect(within(dialog).getByLabelText("Threshold")).toHaveValue("90");
    expect(within(dialog).getByLabelText("Clears at")).toHaveValue("80");
  });

  it("lists maintenance windows, schedules one and snoozes", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/alerts?tab=maintenance");
    const region = await screen.findByRole("region", { name: "Maintenance windows" });
    expect(await within(region).findByText("On now")).toBeVisible();
    expect(within(region).getByText(/Attic AP · Rewiring/)).toBeVisible();

    const form = screen.getByRole("region", { name: "Schedule a window" });
    await user.click(within(form).getByRole("button", { name: "Some devices" }));
    await user.click(within(form).getByRole("button", { name: "Schedule" }));
    expect(within(form).getByText("Pick at least one device.")).toBeVisible();
    await user.click(within(form).getByRole("checkbox", { name: "Gateway" }));
    await user.type(within(form).getByLabelText("Note"), "Firmware");
    await user.click(within(form).getByRole("button", { name: "Schedule" }));
    await waitFor(() => expect(posted("/api/v1/maintenance")).toHaveLength(1));
    const body = posted("/api/v1/maintenance")[0]?.body as {
      nodes: number[];
      note: string;
      starts_at: number;
      ends_at: number;
    };
    expect(body.nodes).toEqual([1]);
    expect(body.note).toBe("Firmware");
    expect(body.ends_at - body.starts_at).toBe(3_600_000);

    await user.click(within(region).getByRole("button", { name: "Snooze alerts" }));
    await user.click(await screen.findByRole("button", { name: "Every device: For 4h" }));
    await waitFor(() => expect(posted("/api/v1/maintenance")).toHaveLength(2));
    const snooze = posted("/api/v1/maintenance")[1]?.body as {
      nodes: number[];
      ends_at: number;
      starts_at: number;
    };
    expect(snooze.nodes).toEqual([]);
    expect(snooze.ends_at - snooze.starts_at).toBe(4 * 3_600_000);

    await user.click(within(region).getAllByRole("button", { name: "End" })[0] as HTMLElement);
    await waitFor(() => expect(posted("/api/v1/maintenance/1")).toHaveLength(1));
  });

  it("lets viewers read but not change", async () => {
    const { ws } = await renderAt("/insights/alerts", "viewer");
    ws.receive({ type: "alerts_snapshot", topic: "alerts", alerts: [alertRecord(1)] });
    const list = await screen.findByRole("region", { name: "Active alerts" });
    await within(list).findByText("Device CPU busy");
    expect(within(list).queryByRole("button", { name: /Acknowledge/ })).not.toBeInTheDocument();
    expect(within(list).queryByRole("button", { name: "Snooze alerts" })).not.toBeInTheDocument();

    await userEvent.setup().click(screen.getByRole("tab", { name: "Rules" }));
    expect(await screen.findByText("Only administrators change rules.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "New rule" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("says so when all is quiet", async () => {
    const { ws } = await renderAt("/insights/alerts");
    ws.receive({ type: "alerts_snapshot", topic: "alerts", alerts: [] });
    expect(await screen.findByText("All quiet")).toBeVisible();
  });
});

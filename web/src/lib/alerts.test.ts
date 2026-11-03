import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { alertRecord } from "../test/fixtures";
import {
  type AlertsState,
  alertPage,
  applyAlerts,
  byAttention,
  causeText,
  eventsAround,
  type Names,
  subjectLabel,
  unattended,
  worstLevel,
} from "./alerts";

const names: Names = {
  node: (id) => ({ 1: "Gateway", 2: "Attic AP", 3: "Garden AP" })[id],
  client: (mac) => (mac === "aa:bb:cc:dd:ee:ff" ? "Phone" : undefined),
};

describe("the active alerts", () => {
  it("keeps those firing newest first: a snapshot replaces, a delta changes, resolved ones leave", () => {
    const [state, setState, dispose] = createRoot(
      (dispose) => [...createStore<AlertsState>({ active: [], ready: false }), dispose] as const,
    );
    const apply = (message: ServerMessage) => {
      setState((draft) => applyAlerts(draft, message));
      flush();
    };
    apply({
      type: "alerts_snapshot",
      topic: "alerts",
      alerts: [alertRecord(1), alertRecord(2)],
    });
    expect(state.ready).toBe(true);
    expect(state.active.map((a) => a.id)).toEqual([2, 1]);
    const kept = state.active[1];

    const ack = { by: 1, username: "admin", at: Date.now() };
    apply({
      type: "alerts_delta",
      topic: "alerts",
      alerts: [
        alertRecord(1, { acknowledged: ack }),
        alertRecord(2, { resolved_at: Date.now(), resolution: "cleared" }),
        alertRecord(3),
      ],
    });
    expect(state.active.map((a) => a.id)).toEqual([3, 1]);
    // An alert already listed keeps its object, changed in place.
    expect(state.active[1]).toBe(kept);
    expect(state.active[1]?.acknowledged).toEqual(ack);

    apply({ type: "alerts_snapshot", topic: "alerts", alerts: [alertRecord(3)] });
    expect(state.active.map((a) => a.id)).toEqual([3]);
    dispose();
  });

  it("counts what wants attention, most severe first", () => {
    const alerts = [
      alertRecord(1, { level: "critical", acknowledged: { by: 1, username: "a", at: 0 } }),
      alertRecord(2, { level: "warning", silenced: true }),
      alertRecord(3, { level: "notice" }),
      alertRecord(4, { level: "warning" }),
    ];
    expect(unattended(alerts).map((a) => a.id)).toEqual([3, 4]);
    expect(worstLevel(unattended(alerts))).toBe("warning");
    expect(worstLevel([])).toBeUndefined();
    expect(byAttention(alerts).map((a) => a.id)).toEqual([4, 3, 1, 2]);
  });
});

describe("alerts in words", () => {
  it("names the subject: a series, a condition, a node, a client or the network", () => {
    const of = (subject: string, node: number | null = null, client: string | null = null) =>
      subjectLabel(alertRecord(1, { subject, node, client }), names);
    expect(of("node:1", 1)).toBe("Gateway");
    expect(of("node:3/radio:radio1", 3)).toBe("Garden AP · radio1");
    expect(of("node:1/wan:eth1", 1)).toBe("Gateway · eth1");
    expect(of("node:2/offline", 2)).toBe("Attic AP");
    expect(of("node:2/mesh:node:2~node:3~wifi", 2)).toContain("Attic AP");
    expect(of("node:2/mesh:node:2~node:3~wifi", 2)).toContain("Garden AP");
    expect(of("client:aa:bb:cc:dd:ee:ff", null, "aa:bb:cc:dd:ee:ff")).toBe("Phone");
    expect(of("site")).toBe("Whole network");
    expect(of("node:9", 9)).toBe("Node 9");
  });

  it("tells the cause: the value beyond the threshold, or how many events matched", () => {
    expect(causeText(alertRecord(1))).toBe("CPU 93 %, above 90 % · worst 97 %");
    expect(
      causeText(
        alertRecord(1, {
          cause: {
            type: "metric",
            metric: "cpu",
            op: "above",
            threshold: 90,
            value: 95,
            worst: 95,
          },
        }),
      ),
    ).toBe("CPU 95 %, above 90 %");
    const event = (matches: number) =>
      alertRecord(1, {
        cause: { type: "event", event: 7, kind: "node_offline", matches, condition: null },
      });
    expect(causeText(event(1))).toBeUndefined();
    expect(causeText(event(3))).toBe("3 matching events");
  });

  it("links to its device or client, and to the events around it", () => {
    expect(alertPage(alertRecord(1, { subject: "node:3/radio:radio0", node: 3 }))).toBe(
      "/devices/3",
    );
    expect(
      alertPage(
        alertRecord(1, {
          subject: "client:aa:bb:cc:dd:ee:ff",
          node: null,
          client: "aa:bb:cc:dd:ee:ff",
        }),
      ),
    ).toBe("/clients/aa:bb:cc:dd:ee:ff");
    expect(alertPage(alertRecord(1, { subject: "site", node: null }))).toBeUndefined();

    const now = 10_000_000;
    const alert = alertRecord(1, { started_at: now - 600_000, resolved_at: now - 120_000 });
    const href = new URL(eventsAround(alert, now), "http://x");
    expect(href.pathname).toBe("/insights/events");
    expect(href.searchParams.get("nodes")).toBe("1");
    expect(Number(href.searchParams.get("from"))).toBe(now - 900_000);
    // Five minutes after it resolved is still to come: up to now.
    expect(Number(href.searchParams.get("to"))).toBe(now);
  });
});

import { describe, expect, it } from "vitest";
import type { RuleView } from "../../api/gen/RuleView";
import type { Names } from "../../lib/alerts";
import { alertRecord } from "../../test/fixtures";
import { admits, filterOf, mergeAlerts, parseHistory } from "./history";
import { checkWindow, emptyDraft, snooze, sortWindows } from "./maintenance";
import { checkDraft, draftOf, parseTarget, ruleSummary, targetText, withChoice } from "./rules";

const names: Names = {
  node: (id) => ({ 1: "Gateway", 3: "Garden AP" })[id],
  client: () => undefined,
};

function rule(when: RuleView["when"], extra: Partial<RuleView> = {}): RuleView {
  return {
    id: 6,
    preset: "cpu",
    modified: false,
    name: "Device CPU busy",
    enabled: true,
    level: "warning",
    when,
    cooldown_s: 900,
    ...extra,
  };
}

describe("rule targets", () => {
  it("reads and writes entity patterns", () => {
    for (const text of [
      "site",
      "node:*",
      "node:3",
      "node:*/radio:*",
      "node:3/radio:radio1",
      "node:*/dev:wan",
      "link:*",
      "client:*",
      "client:aa:bb:cc:dd:ee:ff",
    ]) {
      expect(targetText(parseTarget(text))).toBe(text);
    }
    expect(parseTarget("node:3/dev:*")).toEqual({ kind: "device", node: 3, part: "" });
  });

  it("keeps a value that isn't among the choices", () => {
    expect(withChoice([60, 300], 300)).toEqual([60, 300]);
    expect(withChoice([60, 300], 120)).toEqual([60, 120, 300]);
  });
});

describe("the rule editor's draft", () => {
  it("round-trips a metric rule, and words it", () => {
    const view = rule({
      type: "metric",
      metric: "cpu",
      entities: "node:*",
      op: "above",
      threshold: 90,
      clear: 80,
      for_s: 300,
    });
    const checked = checkDraft(draftOf(view));
    expect(checked).toEqual({
      spec: {
        name: view.name,
        enabled: true,
        level: "warning",
        when: view.when,
        cooldown_s: 900,
      },
    });
    expect(ruleSummary(view, names, () => undefined)).toBe("CPU above 90 % for 5m · Every device");
    const radio = rule({
      ...view.when,
      metric: "busy",
      entities: "node:3/radio:*",
      clear: null,
    } as RuleView["when"]);
    expect(ruleSummary(radio, names, () => undefined)).toBe(
      "Channel busy above 90 % for 5m · Every radio of Garden AP",
    );
  });

  it("round-trips an event rule, and words it", () => {
    const view = rule({
      type: "event",
      kinds: ["log_pattern"],
      categories: [],
      min: null,
      nodes: [1],
      patterns: ["dfs-radar"],
      count: 3,
      window_s: 3600,
    });
    expect(checkDraft(draftOf(view))).toEqual({
      spec: { name: view.name, enabled: true, level: "warning", when: view.when, cooldown_s: 900 },
    });
    expect(ruleSummary(view, names, (id) => (id === "dfs-radar" ? "Radar" : undefined))).toBe(
      "Problem in a log · Radar · 3 times within 1h · on Gateway",
    );
    // Patterns only filter log events: they go when no log event is picked.
    const draft = { ...draftOf(view), kinds: ["node_offline" as const] };
    const spec = checkDraft(draft);
    expect("spec" in spec && spec.spec.when.type === "event" && spec.spec.when.patterns).toEqual(
      [],
    );
  });

  it("refuses what the server would", () => {
    const base = draftOf();
    const errors = (change: Partial<typeof base>) => {
      const checked = checkDraft({ ...base, name: "Busy", ...change });
      return "errors" in checked ? checked.errors : {};
    };
    expect(errors({ name: "  " }).name).toBeDefined();
    expect(errors({ threshold: "lots" }).threshold).toBeDefined();
    expect(errors({ op: "above", threshold: "90", clear: "95" }).clear).toBeDefined();
    expect(errors({ op: "below", threshold: "10", clear: "5" }).clear).toBeDefined();
    expect(errors({ op: "below", threshold: "10", clear: "15" })).toEqual({});
    expect(errors({ target: { kind: "client", node: null, part: "phone" } }).part).toBeDefined();
    expect(errors({ type: "event", count: "0" }).count).toBeDefined();
    expect(errors({ type: "event", count: "2.5" }).count).toBeDefined();
    // A metric the target has no series of becomes its first.
    const checked = checkDraft({
      ...base,
      name: "x",
      target: { kind: "link", node: null, part: "" },
    });
    expect(
      "spec" in checked && checked.spec.when.type === "metric" && checked.spec.when.metric,
    ).toBe("signal");
  });
});

describe("maintenance windows", () => {
  const now = new Date(2026, 10, 2, 10, 0).getTime();

  it("snoozes from now, for a device or every one", () => {
    expect(snooze(3_600_000, 3, now)).toMatchObject({
      starts_at: now,
      ends_at: now + 3_600_000,
      nodes: [3],
    });
    expect(snooze(3_600_000, null, now).nodes).toEqual([]);
  });

  it("checks the form as the server does", () => {
    const draft = emptyDraft(now);
    expect(checkWindow(draft, now)).toEqual({
      window: { starts_at: now, ends_at: now + 3_600_000, nodes: [], note: "" },
    });
    const errors = (change: Partial<typeof draft>) => {
      const checked = checkWindow({ ...draft, ...change }, now);
      return "errors" in checked ? checked.errors : {};
    };
    expect(errors({ ends: draft.starts }).ends).toBe("It ends after it starts.");
    expect(errors({ starts: "2026-10-01T10:00", ends: "2026-10-02T10:00" }).ends).toBe(
      "It ends in the future.",
    );
    expect(errors({ ends: "2026-12-30T10:00" }).ends).toBe("A window lasts at most 30 days.");
    expect(errors({ every: false }).nodes).toBe("Pick at least one device.");
    expect(errors({ starts: "" }).starts).toBe("Pick a date and time.");
  });

  it("lists those on now first, then those to come", () => {
    const w = (id: number, start: number, end: number) => ({
      id,
      starts_at: now + start,
      ends_at: now + end,
      nodes: [],
      note: "",
      created_by: null,
    });
    const sorted = sortWindows(
      [w(1, 7_200_000, 9_000_000), w(2, -60_000, 600_000), w(3, -9_000, -1)],
      now,
    );
    expect(sorted.map((x) => x.id)).toEqual([2, 1]);
  });
});

describe("the alert history", () => {
  it("reads its query from the URL", () => {
    expect(parseHistory({})).toEqual({ min: null, node: null, range: "7d" });
    expect(parseHistory({ min: "warning", node: "3", range: "90d" })).toEqual({
      min: "warning",
      node: 3,
      range: "90d",
    });
    expect(parseHistory({ min: "loud", node: "x", range: "1y" })).toEqual({
      min: null,
      node: null,
      range: "7d",
    });
  });

  it("admits arrivals as the store filters, and merges them by id", () => {
    const now = Date.now();
    const filter = filterOf({ min: "warning", node: 1, range: "24h" }, now);
    expect(admits(filter, alertRecord(1))).toBe(true);
    expect(admits(filter, alertRecord(2, { level: "notice" }))).toBe(false);
    expect(admits(filter, alertRecord(3, { node: 2 }))).toBe(false);
    expect(admits(filter, alertRecord(4, { started_at: now - 2 * 86_400_000 }))).toBe(false);

    const merged = mergeAlerts(
      [alertRecord(1), alertRecord(2)],
      [alertRecord(1, { count: 2 }), alertRecord(3)],
    );
    expect(merged.map((a) => [a.id, a.count])).toEqual([
      [3, 1],
      [2, 1],
      [1, 2],
    ]);
  });
});

import { describe, expect, it } from "vitest";
import type { EventRecord } from "../../api/gen/EventRecord";
import { byDay, dayLabel, ongoing } from "./EventsPage";
import { mergeEvents } from "./feed";
import { filterParams, parseParams, rangeParams, topicOf, windowOf } from "./query";

function event(id: number, at: number, extra: Partial<EventRecord> = {}): EventRecord {
  return {
    id,
    at,
    last_at: at,
    count: 1,
    level: "info",
    category: "client",
    node: 1,
    client: null,
    condition: null,
    what: { kind: "node_adopted" },
    ...extra,
  };
}

describe("the events page's URL", () => {
  it("reads filters and ranges, dropping what it does not know", () => {
    expect(
      parseParams({
        categories: "wan,weather,mesh",
        min: "loud",
        nodes: "3,x,-1",
        clients: "F0:18:98:00:00:01,nope",
        range: "7d",
      }),
    ).toEqual({
      filter: {
        categories: ["wan", "mesh"],
        min: null,
        nodes: [3],
        clients: ["f0:18:98:00:00:01"],
      },
      range: { preset: "7d" },
    });
    expect(parseParams({ from: "100", to: "200" }).range).toEqual({ from: 100, to: 200 });
    expect(parseParams({ range: "1y" }).range).toEqual({ preset: "24h" });
  });

  it("writes them back, and names the topic that carries them", () => {
    const filter = { categories: [], min: "warning" as const, nodes: [3], clients: [] };
    expect(filterParams(filter)).toEqual({
      categories: undefined,
      min: "warning",
      nodes: "3",
      clients: undefined,
    });
    expect(rangeParams({ preset: "24h" })).toEqual({
      range: undefined,
      from: undefined,
      to: undefined,
    });
    expect(topicOf(filter)).toBe("events?node=3&min=warning");
    expect(topicOf({ ...filter, nodes: [3, 4], min: null })).toBe("events");
    expect(windowOf({ preset: "1h" }, 10_000_000)).toEqual({ from: 6_400_000, to: null });
  });
});

describe("the events list", () => {
  it("merges new events and folded repeats by id, newest first", () => {
    const shown = [event(2, 200), event(1, 100)];
    const folded = { ...event(1, 100), count: 4, last_at: 400 };
    const merged = mergeEvents(shown, [event(3, 300), folded]);
    expect(merged.map((e) => e.id)).toEqual([3, 2, 1]);
    expect(merged[2]?.count).toBe(4);
  });

  it("groups by day and finds conditions still raised", () => {
    const day = 86_400_000;
    const now = Date.parse("2026-11-02T12:00:00");
    const events = [event(3, now), event(2, now - 3_600_000), event(1, now - day)];
    expect(byDay(events).map((d) => d.events.length)).toEqual([2, 1]);
    expect(dayLabel(now, now)).toBe("Today");
    expect(dayLabel(now - day, now)).toBe("Yesterday");
    const raised = (id: number, at: number, key: string, up: boolean) =>
      event(id, at, { condition: { key, raised: up } });
    const open = ongoing([
      raised(4, 400, "node:1/offline", false),
      raised(3, 300, "node:1/offline", true),
      raised(2, 200, "node:2/wan:eth1", true),
    ]);
    expect([...open]).toEqual([2]);
  });
});

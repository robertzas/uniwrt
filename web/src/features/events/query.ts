/**
 * What the events page shows, as its URL carries it: a filter (the API's names:
 * `?categories=wan,mesh&min=warning&nodes=3&clients=<mac>`) and a time range, a preset ending
 * now (live) or a fixed window (`from`, `to`), so views can be shared and other pages link here.
 */
import type { EventCategory } from "../../api/gen/EventCategory";
import type { EventLevel } from "../../api/gen/EventLevel";
import type { EventFilter } from "../../lib/api";
import { isCategory, isLevel } from "../../lib/events";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** The preset ranges, each ending now; 60 days is the retention. */
export const EVENT_PRESETS = {
  "1h": HOUR,
  "6h": 6 * HOUR,
  "24h": DAY,
  "7d": 7 * DAY,
  "30d": 30 * DAY,
  "60d": 60 * DAY,
} as const;

export type EventPreset = keyof typeof EVENT_PRESETS;

export const eventPresets = Object.keys(EVENT_PRESETS) as EventPreset[];

export const DEFAULT_PRESET: EventPreset = "24h";

export type EventRange = { preset: EventPreset } | { from: number; to: number };

/** The filter without its times: the range gives them. */
export type Filter = Omit<EventFilter, "from" | "to">;

export interface EventQuery {
  filter: Filter;
  range: EventRange;
}

export type EventParams = {
  categories?: string;
  min?: string;
  nodes?: string;
  clients?: string;
  range?: string;
  from?: string;
  to?: string;
};

export const EMPTY_FILTER: Filter = { categories: [], min: null, nodes: [], clients: [] };

function listOf(text: string | undefined): string[] {
  return [
    ...new Set(
      (text ?? "")
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean),
    ),
  ];
}

const MAC = /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/;

export function parseParams(params: EventParams): EventQuery {
  const categories: EventCategory[] = listOf(params.categories).filter(isCategory);
  const min: EventLevel | null = isLevel(params.min) ? params.min : null;
  const nodes = listOf(params.nodes)
    .map(Number)
    .filter((id) => Number.isSafeInteger(id) && id > 0);
  const clients = listOf(params.clients?.toLowerCase()).filter((mac) => MAC.test(mac));
  const from = Number(params.from);
  const to = Number(params.to);
  const range: EventRange =
    params.from !== undefined && params.to !== undefined && from < to && from > 0
      ? { from, to }
      : {
          preset:
            params.range !== undefined && Object.hasOwn(EVENT_PRESETS, params.range)
              ? (params.range as EventPreset)
              : DEFAULT_PRESET,
        };
  return { filter: { categories, min, nodes, clients }, range };
}

/** The search parameters of a filter; those it leaves empty are removed. */
export function filterParams(filter: Filter): EventParams {
  const join = (values: readonly (string | number)[]) =>
    values.length > 0 ? values.join(",") : undefined;
  return {
    categories: join(filter.categories),
    min: filter.min ?? undefined,
    nodes: join(filter.nodes),
    clients: join(filter.clients),
  } as EventParams;
}

export function rangeParams(range: EventRange): EventParams {
  return "preset" in range
    ? {
        range: range.preset === DEFAULT_PRESET ? undefined : range.preset,
        from: undefined,
        to: undefined,
      }
    : { range: undefined, from: String(range.from), to: String(range.to) };
}

export function isFiltered(filter: Filter): boolean {
  return (
    filter.categories.length > 0 ||
    filter.min !== null ||
    filter.nodes.length > 0 ||
    filter.clients.length > 0
  );
}

export const isLive = (range: EventRange) => "preset" in range;

/** The range's times at `now`: a preset has no end. */
export function windowOf(range: EventRange, now: number): { from: number; to: number | null } {
  return "preset" in range
    ? { from: now - EVENT_PRESETS[range.preset], to: null }
    : { from: range.from, to: range.to };
}

/** The WS topic that carries the filter's new events: narrowed to one node and the level. */
export function topicOf(filter: Filter): string {
  const params = new URLSearchParams();
  const only = filter.nodes.length === 1 ? filter.nodes[0] : undefined;
  if (only !== undefined) params.set("node", String(only));
  if (filter.min !== null && filter.min !== "info") params.set("min", filter.min);
  const query = params.toString();
  return query === "" ? "events" : `events?${query}`;
}

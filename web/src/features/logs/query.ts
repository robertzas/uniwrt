/**
 * What the logs page shows, as its URL carries it: a filter (the API's `LogFilter`, without times)
 * and a time range, a preset ending now (live) or a fixed window (a zoom).
 */
import type { LogFilter } from "../../api/gen/LogFilter";
import type { Severity } from "../../api/gen/Severity";
import { isSeverity } from "./severity";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** The preset ranges, each ending now; 14 days is the retention. */
export const LOG_PRESETS = {
  "15m": 15 * MINUTE,
  "1h": HOUR,
  "6h": 6 * HOUR,
  "24h": DAY,
  "7d": 7 * DAY,
  "14d": 14 * DAY,
} as const;

export type LogPreset = keyof typeof LOG_PRESETS;

export const logPresets = Object.keys(LOG_PRESETS) as LogPreset[];

export const DEFAULT_PRESET: LogPreset = "1h";

export function isLogPreset(value: string | undefined): value is LogPreset {
  return value !== undefined && Object.hasOwn(LOG_PRESETS, value);
}

export type LogRange = { preset: LogPreset } | { from: number; to: number };

/** The filter without its times: the page's range gives them. */
export type Filter = Omit<LogFilter, "from" | "to">;

export interface LogQuery {
  filter: Filter;
  range: LogRange;
}

/** The page's search parameters; the filter's use the API's names (`/insights/logs?nodes=3`). */
export type LogParams = {
  nodes?: string;
  hosts?: string;
  programs?: string;
  min?: string;
  q?: string;
  annotated?: string;
  range?: string;
  from?: string;
  to?: string;
};

export const EMPTY_FILTER: Filter = {
  nodes: [],
  hosts: [],
  programs: [],
  min: null,
  text: null,
  annotated: false,
};

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

export function parseParams(params: LogParams): LogQuery {
  const nodes = listOf(params.nodes)
    .map(Number)
    .filter((id) => Number.isSafeInteger(id) && id > 0);
  const min: Severity | null = isSeverity(params.min) ? params.min : null;
  const text = params.q?.trim() ? params.q : null;
  const from = Number(params.from);
  const to = Number(params.to);
  const range: LogRange =
    params.from && params.to && Number.isFinite(from) && Number.isFinite(to) && to > from
      ? { from, to }
      : { preset: isLogPreset(params.range) ? params.range : DEFAULT_PRESET };
  return {
    filter: {
      nodes,
      hosts: listOf(params.hosts),
      programs: listOf(params.programs),
      min,
      text,
      annotated: params.annotated === "1" || params.annotated === "true",
    },
    range,
  };
}

/** The parameters for a filter; `undefined` removes one (for `setParams`). */
export function filterParams(filter: Filter): LogParams {
  const list = (values: readonly (string | number)[]) =>
    values.length > 0 ? values.join(",") : undefined;
  return {
    nodes: list(filter.nodes),
    hosts: list(filter.hosts),
    programs: list(filter.programs),
    min: filter.min ?? undefined,
    q: filter.text ?? undefined,
    annotated: filter.annotated ? "1" : undefined,
  };
}

export function rangeParams(range: LogRange): LogParams {
  return "preset" in range
    ? { range: range.preset, from: undefined, to: undefined }
    : { range: undefined, from: String(Math.round(range.from)), to: String(Math.round(range.to)) };
}

/** A preset follows new entries; a fixed window stays as it is. */
export function isLive(range: LogRange): boolean {
  return "preset" in range;
}

/** The times the API is asked for: `to` is open for a live range. */
export function windowOf(range: LogRange, now: number): { from: number; to: number | null } {
  return "preset" in range
    ? { from: now - LOG_PRESETS[range.preset], to: null }
    : { from: range.from, to: range.to };
}

export function withWindow(filter: Filter, window: { from: number; to: number | null }): LogFilter {
  return { ...filter, from: window.from, to: window.to };
}

/** Whether the filter narrows anything. */
export function isFiltered(filter: Filter): boolean {
  return (
    filter.nodes.length > 0 ||
    filter.hosts.length > 0 ||
    filter.programs.length > 0 ||
    filter.min !== null ||
    filter.text !== null ||
    filter.annotated
  );
}

/** Mirrors the hub's check (`ws/topic.rs`): a host name a topic may carry. */
const TOPIC_HOST = /^[A-Za-z0-9.\-_:]+$/;

/**
 * The WS topic that carries the entries a filter may admit: narrowed to its one source and its
 * least severity where the hub can; the page filters the rest.
 */
export function topicOf(filter: Filter): string {
  const parts: string[] = [];
  const [node] = filter.nodes;
  const [host] = filter.hosts;
  if (filter.nodes.length + filter.hosts.length === 1) {
    if (node !== undefined) parts.push(`node=${node}`);
    else if (host !== undefined && TOPIC_HOST.test(host)) parts.push(`host=${host}`);
  }
  if (filter.min !== null && filter.min !== "debug") parts.push(`min=${filter.min}`);
  return parts.length > 0 ? `logs?${parts.join("&")}` : "logs";
}

/** A saved search keeps what to find, not when: the range stays the page's. */
export function savedFilter(filter: Filter): LogFilter {
  return { ...filter, from: null, to: null };
}

/** Two filters find the same entries (for marking the saved search in use). */
export function sameFilter(a: Filter, b: Filter): boolean {
  const set = (values: readonly (string | number)[]) => [...values].map(String).sort().join(",");
  return (
    set(a.nodes) === set(b.nodes) &&
    set(a.hosts) === set(b.hosts) &&
    set(a.programs) === set(b.programs) &&
    a.min === b.min &&
    (a.text?.trim() ?? "") === (b.text?.trim() ?? "") &&
    a.annotated === b.annotated
  );
}

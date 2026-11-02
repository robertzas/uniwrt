import type { AdoptBody } from "../api/gen/AdoptBody";
import type { AuthState } from "../api/gen/AuthState";
import type { BatchAdoptBody } from "../api/gen/BatchAdoptBody";
import type { BatchAdoptResult } from "../api/gen/BatchAdoptResult";
import type { Client } from "../api/gen/Client";
import type { ClientDetail } from "../api/gen/ClientDetail";
import type { ClientPatch } from "../api/gen/ClientPatch";
import type { ClientSession } from "../api/gen/ClientSession";
import type { Credentials } from "../api/gen/Credentials";
import type { DiscoveryReport } from "../api/gen/DiscoveryReport";
import type { EventCategory } from "../api/gen/EventCategory";
import type { EventLevel } from "../api/gen/EventLevel";
import type { EventPage } from "../api/gen/EventPage";
import type { Health } from "../api/gen/Health";
import type { Job } from "../api/gen/Job";
import type { LogContext } from "../api/gen/LogContext";
import type { LogFilter } from "../api/gen/LogFilter";
import type { LogHistogram } from "../api/gen/LogHistogram";
import type { LogPage } from "../api/gen/LogPage";
import type { LogPatterns } from "../api/gen/LogPatterns";
import type { LogSourceList } from "../api/gen/LogSourceList";
import type { Metric } from "../api/gen/Metric";
import type { MetricsResponse } from "../api/gen/MetricsResponse";
import type { NodeDetail } from "../api/gen/NodeDetail";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { PinRequest } from "../api/gen/PinRequest";
import type { ProbeRequest } from "../api/gen/ProbeRequest";
import type { ProbeResult } from "../api/gen/ProbeResult";
import type { SavedSearch } from "../api/gen/SavedSearch";
import type { SavedSearches } from "../api/gen/SavedSearches";
import type { SeriesInfo } from "../api/gen/SeriesInfo";
import type { Topology } from "../api/gen/Topology";
import type { TopologyPin } from "../api/gen/TopologyPin";
import type { TopologyTimeline } from "../api/gen/TopologyTimeline";
import type { UserInfo } from "../api/gen/UserInfo";

/** A failed API call. Problem details (RFC 9457) from the server fill `title` and `message`. */
export class ApiError extends Error {
  override readonly name = "ApiError";

  constructor(
    readonly status: number,
    readonly title: string,
    detail?: string,
    /** Seconds to wait before trying again, from `Retry-After` (429). */
    readonly retryAfter?: number,
  ) {
    super(detail ?? title);
  }
}

let sessionEnded: (() => void) | undefined;

/**
 * Called when a request that needs a session gets 401: the session expired or was revoked. The
 * app answers by showing the sign-in screen. Returns a function that removes the handler.
 */
export function onSessionEnded(handler: () => void): () => void {
  sessionEnded = handler;
  return () => {
    if (sessionEnded === handler) sessionEnded = undefined;
  };
}

interface Options {
  method?: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  body?: unknown;
  signal?: AbortSignal | undefined;
  /** A 401 means the session ended (default); false for sign-in, whose 401 is a wrong password. */
  session?: boolean;
}

async function request<T>(path: string, options: Options = {}): Promise<T> {
  const method = options.method ?? "GET";
  const headers = new Headers({ accept: "application/json" });
  // Every state-changing request carries the CSRF header (security.md).
  if (method !== "GET") headers.set("x-uniwrt-csrf", "1");
  if (options.body !== undefined) headers.set("content-type", "application/json");
  const response = await fetch(`/api/v1${path}`, {
    method,
    headers,
    ...(options.body !== undefined && { body: JSON.stringify(options.body) }),
    ...(options.signal && { signal: options.signal }),
  });
  if (!response.ok) {
    if (response.status === 401 && options.session !== false) sessionEnded?.();
    throw await toError(response);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function toError(response: Response): Promise<ApiError> {
  const retry = Number(response.headers.get("retry-after"));
  const retryAfter = Number.isFinite(retry) && retry > 0 ? retry : undefined;
  if (response.headers.get("content-type")?.startsWith("application/problem+json")) {
    const problem = (await response.json()) as { title?: string; detail?: string };
    return new ApiError(
      response.status,
      problem.title ?? response.statusText,
      problem.detail,
      retryAfter,
    );
  }
  const title = response.statusText || `HTTP ${response.status}`;
  return new ApiError(response.status, title, undefined, retryAfter);
}

/** What `api.metrics.get` reads: an entity's metrics over a time range (ms). */
export interface MetricsQuery {
  entity: string;
  metrics: readonly Metric[];
  from: number;
  to: number;
  width: number;
}

/** A log filter as the logs routes read it (`crates/api/src/logs.rs`); empty parts left out. */
export function logParams(filter: LogFilter): URLSearchParams {
  const params = new URLSearchParams();
  const list = (name: string, values: readonly (string | number)[]) => {
    if (values.length > 0) params.set(name, values.join(","));
  };
  list("nodes", filter.nodes);
  list("hosts", filter.hosts);
  list("programs", filter.programs);
  if (filter.min !== null) params.set("min", filter.min);
  if (filter.text !== null && filter.text.trim() !== "") params.set("q", filter.text);
  if (filter.annotated) params.set("annotated", "true");
  if (filter.from !== null) params.set("from", String(Math.round(filter.from)));
  if (filter.to !== null) params.set("to", String(Math.round(filter.to)));
  return params;
}

/** Which events to read, as `GET v1/events` takes them (`model::events::EventFilter`). */
export interface EventFilter {
  categories: EventCategory[];
  /** The least that matters to include: `warning` keeps warnings and critical ones. */
  min: EventLevel | null;
  nodes: number[];
  clients: string[];
  from: number | null;
  to: number | null;
}

/** An event filter as the events route reads it (`crates/api/src/events.rs`); empty parts left out. */
export function eventParams(filter: EventFilter): URLSearchParams {
  const params = new URLSearchParams();
  const list = (name: string, values: readonly (string | number)[]) => {
    if (values.length > 0) params.set(name, values.join(","));
  };
  list("categories", filter.categories);
  list("nodes", filter.nodes);
  list("clients", filter.clients);
  if (filter.min !== null) params.set("min", filter.min);
  if (filter.from !== null) params.set("from", String(Math.round(filter.from)));
  if (filter.to !== null) params.set("to", String(Math.round(filter.to)));
  return params;
}

/** The typed API client; components reach the server only through this (CONVENTIONS.md). */
export const api = {
  health: (signal?: AbortSignal) => request<Health>("/health", { signal }),
  auth: {
    state: (signal?: AbortSignal) => request<AuthState>("/auth", { signal, session: false }),
    setup: (credentials: Credentials) =>
      request<UserInfo>("/auth/setup", { method: "POST", body: credentials, session: false }),
    login: (credentials: Credentials) =>
      request<UserInfo>("/auth/login", { method: "POST", body: credentials, session: false }),
    logout: () => request<void>("/auth/logout", { method: "POST", session: false }),
  },
  nodes: {
    list: (signal?: AbortSignal) => request<NodeSummary[]>("/nodes", { signal }),
    get: (id: NodeId, signal?: AbortSignal) => request<NodeDetail>(`/nodes/${id}`, { signal }),
  },
  adopt: {
    /** Reads the node's host key, for the user to confirm before adopting it. */
    probe: (target: ProbeRequest) =>
      request<ProbeResult>("/adopt/probe", { method: "POST", body: target }),
    /** Starts adopting a node; the job that answers is followed on the WS topic `jobs`. */
    start: (body: AdoptBody) => request<Job>("/adopt", { method: "POST", body }),
    batch: (body: BatchAdoptBody) =>
      request<BatchAdoptResult>("/adopt/batch", { method: "POST", body }),
  },
  /** Live clients come over the WS topics `clients` and `client:<mac>`. */
  clients: {
    list: (signal?: AbortSignal) => request<Client[]>("/clients", { signal }),
    get: (mac: string, signal?: AbortSignal) =>
      request<ClientDetail>(`/clients/${encodeURIComponent(mac)}`, { signal }),
    /** The client's sessions that went on at or after `since` (ms), oldest first. */
    sessions: (mac: string, since: number, signal?: AbortSignal) =>
      request<ClientSession[]>(
        `/clients/${encodeURIComponent(mac)}/sessions?since=${Math.round(since)}`,
        { signal },
      ),
    /** Gives the client an alias, or takes it away (blank or null); admins only. */
    rename: (mac: string, patch: ClientPatch) =>
      request<Client>(`/clients/${encodeURIComponent(mac)}`, { method: "PATCH", body: patch }),
  },
  /** Stored time series (observability.md, Metrics). */
  metrics: {
    /** An entity's series over `from`–`to` (ms), each reduced to about `width` points. */
    get: (query: MetricsQuery, signal?: AbortSignal) => {
      const params = new URLSearchParams({
        entity: query.entity,
        metrics: query.metrics.join(","),
        from: String(Math.round(query.from)),
        to: String(Math.round(query.to)),
        width: String(Math.round(query.width)),
      });
      return request<MetricsResponse>(`/metrics?${params}`, { signal });
    },
    /** The series kept, for every entity, or for `under` and the entities below it. */
    series: (under?: string, signal?: AbortSignal) =>
      request<SeriesInfo[]>(
        under === undefined
          ? "/metrics/series"
          : `/metrics/series?under=${encodeURIComponent(under)}`,
        { signal },
      ),
  },
  /** Stored log entries (observability.md, Logs); new ones come over the WS topic `logs`. */
  logs: {
    /** Entries newest first, `limit` at a time; `before` is the previous page's `next`. */
    page: (filter: LogFilter, before: string | null, limit: number, signal?: AbortSignal) => {
      const params = logParams(filter);
      if (before !== null) params.set("before", before);
      params.set("limit", String(limit));
      return request<LogPage>(`/logs?${params}`, { signal });
    },
    /** How many entries each of `buckets` spans of the filter's range holds, by severity. */
    histogram: (filter: LogFilter, buckets: number, signal?: AbortSignal) => {
      const params = logParams(filter);
      params.set("buckets", String(Math.round(buckets)));
      return request<LogHistogram>(`/logs/histogram?${params}`, { signal });
    },
    /** The entry with `around` entries before and after it, from every source, oldest first. */
    context: (id: number, around: number, signal?: AbortSignal) =>
      request<LogContext>(`/logs/${id}/context?around=${Math.round(around)}`, { signal }),
    /** The hosts and programs that logged lately. */
    sources: (signal?: AbortSignal) => request<LogSourceList>("/logs/sources", { signal }),
    /** What each annotation id means. */
    patterns: (signal?: AbortSignal) => request<LogPatterns>("/logs/patterns", { signal }),
    /** Saved searches, shared by every user. */
    searches: (signal?: AbortSignal) => request<SavedSearches>("/logs/searches", { signal }),
    save: (name: string, filter: LogFilter) =>
      request<SavedSearch>("/logs/searches", { method: "POST", body: { name, filter } }),
    remove: (id: number) => request<void>(`/logs/searches/${id}`, { method: "DELETE" }),
  },
  /** Recorded events (observability.md, Events); new ones come over the WS topic `events`. */
  events: {
    /** Events newest first, `limit` at a time; `before` is the previous page's `next`. */
    page: (filter: EventFilter, before: string | null, limit: number, signal?: AbortSignal) => {
      const params = eventParams(filter);
      if (before !== null) params.set("before", before);
      params.set("limit", String(limit));
      return request<EventPage>(`/events?${params}`, { signal });
    },
  },
  /** The unadopted OpenWrt devices the adopted nodes see; waits up to 3 s for new probes. */
  discovered: (signal?: AbortSignal) => request<DiscoveryReport>("/discovered", { signal }),
  /** The live topology comes over the WS topic `topology`; these serve its replay and layout. */
  topology: {
    /** How far back the replay goes, and when the topology changed. */
    timeline: (signal?: AbortSignal) => request<TopologyTimeline>("/topology/timeline", { signal }),
    /** The topology at a time, as the history keeps it (no traffic or load). */
    at: (ms: number, signal?: AbortSignal) =>
      request<Topology>(`/topology/at/${Math.round(ms)}`, { signal }),
    /** The pinned vertices, shared by every user and browser. */
    layout: (signal?: AbortSignal) => request<TopologyPin[]>("/topology/layout", { signal }),
    pin: (vertex: string, at: PinRequest) =>
      request<void>(`/topology/layout/${encodeURIComponent(vertex)}`, { method: "PUT", body: at }),
    unpin: (vertex: string) =>
      request<void>(`/topology/layout/${encodeURIComponent(vertex)}`, { method: "DELETE" }),
    unpinAll: () => request<void>("/topology/layout", { method: "DELETE" }),
  },
};

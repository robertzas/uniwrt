import type { AdoptBody } from "../api/gen/AdoptBody";
import type { AlertPage } from "../api/gen/AlertPage";
import type { AlertRecord } from "../api/gen/AlertRecord";
import type { ApplyBody } from "../api/gen/ApplyBody";
import type { ApplyPlan } from "../api/gen/ApplyPlan";
import type { AuthState } from "../api/gen/AuthState";
import type { BatchAdoptBody } from "../api/gen/BatchAdoptBody";
import type { BatchAdoptResult } from "../api/gen/BatchAdoptResult";
import type { Client } from "../api/gen/Client";
import type { ClientDetail } from "../api/gen/ClientDetail";
import type { ClientKick } from "../api/gen/ClientKick";
import type { ClientPatch } from "../api/gen/ClientPatch";
import type { ClientSession } from "../api/gen/ClientSession";
import type { Credentials } from "../api/gen/Credentials";
import type { DiscoveryReport } from "../api/gen/DiscoveryReport";
import type { DriftMuteBody } from "../api/gen/DriftMuteBody";
import type { DriftState } from "../api/gen/DriftState";
import type { DriftTarget } from "../api/gen/DriftTarget";
import type { EventCategory } from "../api/gen/EventCategory";
import type { EventLevel } from "../api/gen/EventLevel";
import type { EventPage } from "../api/gen/EventPage";
import type { FoundGateways } from "../api/gen/FoundGateways";
import type { Health } from "../api/gen/Health";
import type { ImportAccept } from "../api/gen/ImportAccept";
import type { ImportPreview } from "../api/gen/ImportPreview";
import type { ImportRequest } from "../api/gen/ImportRequest";
import type { Issue } from "../api/gen/Issue";
import type { Job } from "../api/gen/Job";
import type { KickOutcome } from "../api/gen/KickOutcome";
import type { LogContext } from "../api/gen/LogContext";
import type { LogFilter } from "../api/gen/LogFilter";
import type { LogHistogram } from "../api/gen/LogHistogram";
import type { LogPage } from "../api/gen/LogPage";
import type { LogPatterns } from "../api/gen/LogPatterns";
import type { LogSourceList } from "../api/gen/LogSourceList";
import type { MaintenanceWindow } from "../api/gen/MaintenanceWindow";
import type { Metric } from "../api/gen/Metric";
import type { MetricsResponse } from "../api/gen/MetricsResponse";
import type { Network } from "../api/gen/Network";
import type { NewWindow } from "../api/gen/NewWindow";
import type { NodeDetail } from "../api/gen/NodeDetail";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { NotificationSettings } from "../api/gen/NotificationSettings";
import type { NotifierInput } from "../api/gen/NotifierInput";
import type { NotifierList } from "../api/gen/NotifierList";
import type { NotifierTest } from "../api/gen/NotifierTest";
import type { NotifierView } from "../api/gen/NotifierView";
import type { PinRequest } from "../api/gen/PinRequest";
import type { Preset } from "../api/gen/Preset";
import type { ProbeRequest } from "../api/gen/ProbeRequest";
import type { ProbeResult } from "../api/gen/ProbeResult";
import type { Purpose } from "../api/gen/Purpose";
import type { ReservationSuggestion } from "../api/gen/ReservationSuggestion";
import type { RestoreBody } from "../api/gen/RestoreBody";
import type { RevealedSecret } from "../api/gen/RevealedSecret";
import type { RevealSecret } from "../api/gen/RevealSecret";
import type { RevisionDiff } from "../api/gen/RevisionDiff";
import type { RevisionPage } from "../api/gen/RevisionPage";
import type { RfSettings } from "../api/gen/RfSettings";
import type { RfView } from "../api/gen/RfView";
import type { RuleList } from "../api/gen/RuleList";
import type { RuleSpec } from "../api/gen/RuleSpec";
import type { RuleView } from "../api/gen/RuleView";
import type { SavedSearch } from "../api/gen/SavedSearch";
import type { SavedSearches } from "../api/gen/SavedSearches";
import type { Scan } from "../api/gen/Scan";
import type { ScanRequest } from "../api/gen/ScanRequest";
import type { ScanSummary } from "../api/gen/ScanSummary";
import type { ScansQueued } from "../api/gen/ScansQueued";
import type { SeriesInfo } from "../api/gen/SeriesInfo";
import type { SiteSave } from "../api/gen/SiteSave";
import type { SiteView } from "../api/gen/SiteView";
import type { SpeedTestBody } from "../api/gen/SpeedTestBody";
import type { SpeedTests } from "../api/gen/SpeedTests";
import type { Topology } from "../api/gen/Topology";
import type { TopologyPin } from "../api/gen/TopologyPin";
import type { TopologyTimeline } from "../api/gen/TopologyTimeline";
import type { UserInfo } from "../api/gen/UserInfo";
import type { WanTestBody } from "../api/gen/WanTestBody";
import type { WindowList } from "../api/gen/WindowList";

/** A failed API call. Problem details (RFC 9457) from the server fill `title` and `message`. */
export class ApiError extends Error {
  override readonly name = "ApiError";

  constructor(
    readonly status: number,
    readonly title: string,
    detail?: string,
    /** Seconds to wait before trying again, from `Retry-After` (429). */
    readonly retryAfter?: number,
    /** A refused document's issues by field (422). */
    readonly issues: readonly Issue[] = [],
  ) {
    super(detail ?? title);
  }
}

/** The message of a failed call, for a toast or a form. */
export function errorText(error: unknown): string | undefined {
  return error instanceof Error ? error.message : undefined;
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
    const problem = (await response.json()) as {
      title?: string;
      detail?: string;
      issues?: Issue[];
    };
    return new ApiError(
      response.status,
      problem.title ?? response.statusText,
      problem.detail,
      retryAfter,
      problem.issues ?? [],
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
  /** Of these kinds only, such as `client_steered` (none: every kind). */
  kinds?: string[];
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
  list("kinds", filter.kinds ?? []);
  list("nodes", filter.nodes);
  list("clients", filter.clients);
  if (filter.min !== null) params.set("min", filter.min);
  if (filter.from !== null) params.set("from", String(Math.round(filter.from)));
  if (filter.to !== null) params.set("to", String(Math.round(filter.to)));
  return params;
}

/** Which alerts to read, as `GET v1/alerts` takes them (`model::alerts::AlertFilter`). */
export interface AlertFilter {
  /** Only those still firing. */
  active: boolean;
  /** The least that matters to include. */
  min: EventLevel | null;
  nodes: number[];
  rules: number[];
  /** When they started. */
  from: number | null;
  to: number | null;
}

/** An alert filter as the alerts route reads it (`crates/api/src/alerts`); empty parts left out. */
export function alertParams(filter: AlertFilter): URLSearchParams {
  const params = new URLSearchParams();
  if (filter.active) params.set("active", "true");
  if (filter.min !== null) params.set("min", filter.min);
  if (filter.nodes.length > 0) params.set("nodes", filter.nodes.join(","));
  if (filter.rules.length > 0) params.set("rules", filter.rules.join(","));
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
    /** Starts the job that turns management on or off (admins); the `jobs` topic follows it. */
    manage: (id: NodeId, enabled: boolean) =>
      request<Job>(`/nodes/${id}/management`, { method: "PUT", body: { enabled } }),
    /** Starts the job that swaps a managed node's basic wpad for the full one (admins, D-089). */
    wpad: (id: NodeId) => request<Job>(`/nodes/${id}/wpad`, { method: "POST" }),
    /** The root password the vault keeps for the node (admins; audited); 404 when it keeps none. */
    rootPassword: (id: NodeId) =>
      request<RevealedSecret>(`/nodes/${id}/root-password/reveal`, { method: "POST" }),
  },
  /** First run (adoption.md, Path E): the routers that could be the gateway to set up (admins). */
  bootstrap: {
    gateways: (signal?: AbortSignal) => request<FoundGateways>("/bootstrap/gateway", { signal }),
  },
  /** The site's desired state (lifecycle.md, Desired state & revisions). */
  site: {
    get: (signal?: AbortSignal) => request<SiteView>("/site", { signal }),
    /** Saves the next revision (admins); 409 when `base` is no longer the latest. */
    save: (save: SiteSave) => request<SiteView>("/site", { method: "PUT", body: save }),
    /** A secret of the latest revision by its path, such as a WLAN's passphrase (admins; audited). */
    reveal: (path: string) => {
      const body: RevealSecret = { path };
      return request<RevealedSecret>("/site/secrets/reveal", { method: "POST", body });
    },
  },
  /** The address plan's suggestions (network.md, Address plan; D-077); nothing is saved. */
  ipam: {
    /**
     * A new network of `purpose` on `vlan` or its usual VLAN, with its /24 and pool; 409 when
     * the VLAN is taken or no VLAN or /24 is free.
     */
    network: (purpose: Exclude<Purpose, "default">, vlan?: number) =>
      request<Network>(
        `/ipam/network?purpose=${purpose}${vlan === undefined ? "" : `&vlan=${vlan}`}`,
      ),
    /**
     * The address to reserve for a client ("Fix IP"): the reservation `mac` has already, else
     * on the network holding `ip`; 404 when no network serves `ip`, 409 when it is full.
     */
    reservation: (ip: string, mac?: string) =>
      request<ReservationSuggestion>(
        `/ipam/reservation?ip=${encodeURIComponent(ip)}${mac ? `&mac=${encodeURIComponent(mac)}` : ""}`,
      ),
  },
  /** The WAN wizard (wan.md; D-075). */
  wan: {
    /** Every ISP preset, generic ones first, then by country and name. */
    presets: (signal?: AbortSignal) => request<Preset[]>("/isp/presets", { signal }),
    /**
     * Tries a WAN on the managed gateway and rolls it back (admins); the `jobs` topic follows
     * the job. 422 with the WAN's issues, 409 while the gateway is in observe mode.
     */
    test: (body: WanTestBody) => request<Job>("/wan/test", { method: "POST", body }),
    /**
     * Measures the line from the gateway (admins; D-076); `bypass_sqm` stops SQM for it (a
     * managed gateway, else 409). The `jobs` topic follows the job.
     */
    speedTest: (body: SpeedTestBody) => request<Job>("/wan/speed-test", { method: "POST", body }),
    /** The speed tests, newest first, with what the newest test of the line suggests. */
    speedTests: (signal?: AbortSignal, limit = 20) =>
      request<SpeedTests>(`/wan/speed-tests?limit=${limit}`, { signal }),
  },
  /** Applying the site's latest revision (lifecycle.md, Render → diff → apply); admins only. */
  apply: {
    /**
     * What applying the latest revision would do to each node, read fresh from every managed
     * node (it logs in to each, so it takes seconds); nothing changes on them.
     */
    plan: (signal?: AbortSignal) => request<ApplyPlan>("/apply/plan", { signal }),
    /** Starts applying `revision`, which must still be the latest (409 otherwise); the `jobs` topic follows it. */
    start: (revision: number) => {
      const body: ApplyBody = { revision };
      return request<Job>("/apply", { method: "POST", body });
    },
  },
  /** Drift (lifecycle.md, Drift; D-074); acknowledging and muting are admins'. */
  drift: {
    get: (signal?: AbortSignal) => request<DriftState>("/drift", { signal }),
    /** Accepts the drift as it is until the file changes again; 404 without such a drift. */
    acknowledge: (node: NodeId, pkg: string) =>
      request<void>(`/nodes/${node}/drift/${encodeURIComponent(pkg)}/acknowledge`, {
        method: "POST",
      }),
    /** Stops watching the package (`true`) or watches it again. */
    mute: (node: NodeId, pkg: string, muted: boolean) => {
      const body: DriftMuteBody = { muted };
      return request<void>(`/nodes/${node}/drift/${encodeURIComponent(pkg)}/mute`, {
        method: "PUT",
        body,
      });
    },
  },
  /** Brownfield import (lifecycle.md, Brownfield import; D-073); admins only. A drift's import
   * reads only that package as its node holds it (D-074). */
  import: {
    /**
     * Reads every adopted node (it logs in to each, so it takes seconds; nothing changes on
     * them) and what importing them into the latest revision would change; the controller holds
     * it for 15 minutes.
     */
    preview: (signal?: AbortSignal, drift?: DriftTarget) => {
      const body: ImportRequest | undefined = drift ? { drift } : undefined;
      return request<ImportPreview>("/import/preview", { method: "POST", body, signal });
    },
    /** Saves the held preview `id` as the next revision: 404 once expired, 409 when stale. */
    accept: (id: string, message: string | null) => {
      const body: ImportAccept = { id, message };
      return request<SiteView>("/import/accept", { method: "POST", body });
    },
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
    /** Deauthenticates the client on every managed access point, keeping it off a while; admins. */
    kick: (mac: string, body: ClientKick) =>
      request<KickOutcome>(`/clients/${encodeURIComponent(mac)}/kick`, { method: "POST", body }),
    /** Keeps the client off every managed access point until unblocked; admins only. */
    block: (mac: string) =>
      request<Client>(`/clients/${encodeURIComponent(mac)}/block`, { method: "PUT" }),
    unblock: (mac: string) =>
      request<Client>(`/clients/${encodeURIComponent(mac)}/block`, { method: "DELETE" }),
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
  /** Alerts (observability.md, Alerts); the active ones come over the WS topic `alerts`. */
  alerts: {
    /** Alerts newest first by start, `limit` at a time; `before` is the previous page's `next`. */
    page: (filter: AlertFilter, before: string | null, limit: number, signal?: AbortSignal) => {
      const params = alertParams(filter);
      if (before !== null) params.set("before", before);
      params.set("limit", String(limit));
      return request<AlertPage>(`/alerts?${params}`, { signal });
    },
    /** Acknowledges an alert still firing; admins only. */
    acknowledge: (id: number) =>
      request<AlertRecord>(`/alerts/${id}/acknowledge`, { method: "POST" }),
  },
  /** Alert rules, built in and the user's; changes are admins'. */
  alertRules: {
    list: (signal?: AbortSignal) => request<RuleList>("/alert-rules", { signal }),
    create: (spec: RuleSpec) => request<RuleView>("/alert-rules", { method: "POST", body: spec }),
    update: (id: number, spec: RuleSpec) =>
      request<RuleView>(`/alert-rules/${id}`, { method: "PUT", body: spec }),
    /** Deletes one of the user's rules (built-in ones are disabled instead). */
    remove: (id: number) => request<void>(`/alert-rules/${id}`, { method: "DELETE" }),
    /** Puts a built-in rule back as it shipped. */
    reset: (id: number) => request<RuleView>(`/alert-rules/${id}/reset`, { method: "POST" }),
  },
  /** Maintenance windows, on now or to come, which silence alerts; changes are admins'. */
  maintenance: {
    list: (signal?: AbortSignal) => request<WindowList>("/maintenance", { signal }),
    create: (window: NewWindow) =>
      request<MaintenanceWindow>("/maintenance", { method: "POST", body: window }),
    remove: (id: number) => request<void>(`/maintenance/${id}`, { method: "DELETE" }),
  },
  /** Where alerts are sent (ntfy, webhooks, email) and the settings they share; changes are admins'. */
  notifiers: {
    list: (signal?: AbortSignal) => request<NotifierList>("/notifiers", { signal }),
    create: (input: NotifierInput) =>
      request<NotifierView>("/notifiers", { method: "POST", body: input }),
    /** Secrets left out are kept, `null` removes one. */
    update: (id: number, input: NotifierInput) =>
      request<NotifierView>(`/notifiers/${id}`, { method: "PUT", body: input }),
    remove: (id: number) => request<void>(`/notifiers/${id}`, { method: "DELETE" }),
    /** Sends a test at once; a 502's detail says why it failed. */
    test: (test: NotifierTest) => request<void>("/notifiers/test", { method: "POST", body: test }),
    settings: (settings: NotificationSettings) =>
      request<NotificationSettings>("/notifications/settings", { method: "PUT", body: settings }),
  },
  /** The site's history (lifecycle.md, Desired state & revisions). */
  revisions: {
    /** Revisions newest first, `limit` at a time; `before` is the last one's id. */
    page: (before: number | null, limit: number, signal?: AbortSignal) => {
      const params = new URLSearchParams({ limit: String(limit) });
      if (before !== null) params.set("before", String(before));
      return request<RevisionPage>(`/revisions?${params}`, { signal });
    },
    /** The changes from `against` (its parent when `null`) to revision `id`. */
    diff: (id: number, against: number | null, signal?: AbortSignal) =>
      request<RevisionDiff>(
        `/revisions/${id}/diff${against === null ? "" : `?against=${against}`}`,
        { signal },
      ),
    /** Makes revision `id`'s document the next revision (admins); 409 when `base` is no longer the latest. */
    restore: (id: number, body: RestoreBody) =>
      request<SiteView>(`/revisions/${id}/restore`, { method: "POST", body }),
  },
  /** RF insight (wifi.md); the view itself comes over the WS topic `rf`. */
  rf: {
    get: (signal?: AbortSignal) => request<RfView>("/rf", { signal }),
    /** Queues scans of every radio, a node's, or one of its radios; admins only. */
    scan: (body: ScanRequest) => request<ScansQueued>("/rf/scans", { method: "POST", body }),
    /** Stored scans, newest first, without what they heard. */
    scans: (query: { node?: number; radio?: string; limit?: number }, signal?: AbortSignal) => {
      const params = new URLSearchParams();
      if (query.node !== undefined) params.set("node", String(query.node));
      if (query.radio !== undefined) params.set("radio", query.radio);
      if (query.limit !== undefined) params.set("limit", String(query.limit));
      return request<ScanSummary[]>(`/rf/scans?${params}`, { signal });
    },
    scanById: (id: number, signal?: AbortSignal) => request<Scan>(`/rf/scans/${id}`, { signal }),
    settings: (settings: RfSettings) =>
      request<RfSettings>("/rf/settings", { method: "PUT", body: settings }),
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

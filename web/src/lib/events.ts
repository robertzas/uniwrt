/**
 * Events in words and pictures (features/observability.md, Events): a title and a detail line
 * for each kind, the icon of each category, the tone of each level, and the logs around an
 * event. Shared by the events page and the dashboard.
 */
import { Globe, Network, Server, ShieldAlert, Smartphone, Waypoints, Wifi } from "lucide";
import type { EventCategory } from "../api/gen/EventCategory";
import type { EventLevel } from "../api/gen/EventLevel";
import type { EventRecord } from "../api/gen/EventRecord";
import type { StatusReason } from "../api/gen/StatusReason";
import type { EventFilter } from "./api";
import { formatBytes, formatDuration, formatRate } from "./format";
import { strings } from "./strings";

const t = strings.events;

export const LEVELS: readonly EventLevel[] = ["critical", "warning", "notice", "info"];

export const CATEGORIES: readonly EventCategory[] = [
  "system",
  "wan",
  "mesh",
  "wifi",
  "client",
  "network",
  "security",
];

export function isLevel(value: string | undefined): value is EventLevel {
  return value !== undefined && (LEVELS as readonly string[]).includes(value);
}

export function isCategory(value: string): value is EventCategory {
  return (CATEGORIES as readonly string[]).includes(value);
}

/** The icon of each category (lucide data, rendered by `ui/Icon`). */
export const categoryIcons: Record<EventCategory, typeof Server> = {
  system: Server,
  wan: Globe,
  mesh: Waypoints,
  wifi: Wifi,
  client: Smartphone,
  network: Network,
  security: ShieldAlert,
};

/** The tone of each level, as `ui/Badge` names them. */
export const levelTones: Record<EventLevel, "crit" | "warn" | "info" | "neutral"> = {
  critical: "crit",
  warning: "warn",
  notice: "info",
  info: "neutral",
};

/** How to name what events point at. */
export interface Names {
  node(id: number): string;
  client(mac: string): string;
  /** A log pattern's meaning, when known. */
  pattern(id: string): string | undefined;
}

export interface Described {
  title: string;
  /** A second line, when there is more to say. */
  detail?: string;
}

const duration = (ms: number | null) => (ms === null ? null : formatDuration(ms / 1000));

/** A status reason in words, without its retry time (the event is past). */
function reasonText(reason: StatusReason | undefined): string | undefined {
  const r = strings.devices.reason;
  switch (reason?.kind) {
    case undefined:
      return undefined;
    case "connecting":
      return r.connecting(reason.attempt);
    case "unreachable":
      return r.unreachable(reason.error);
    case "host_key_mismatch":
      return r.hostKeyMismatch;
    case "stopped":
      return r.stopped;
    case "no_agent":
      return r.noAgent;
    case "agent_starting":
      return r.agentStarting;
    case "agent_failed":
      return r.agentFailed(reason.error);
    case "request_failed":
      return r.requestFailed(reason.op, reason.error);
    case "telemetry_stale":
      return r.telemetryStale;
    case "collect_failed":
      return r.collectFailed(reason.set, reason.error);
  }
}

const withDetail = (title: string, detail: string | null | undefined): Described =>
  detail ? { title, detail } : { title };

/** What happened, for people. */
export function describeEvent(event: EventRecord, names: Names): Described {
  const titles = t.titles;
  const d = t.detail;
  const placed = event.node === null ? null : names.node(event.node);
  const node = placed ?? t.controller;
  const client = event.client === null ? "" : names.client(event.client);
  const what = event.what;
  switch (what.kind) {
    case "controller_started":
      return { title: titles.controllerStarted(what.version) };
    case "node_adopted":
      return { title: titles.nodeAdopted(node) };
    case "node_offline":
      return withDetail(titles.nodeOffline(node), reasonText(what.reasons[0]));
    case "node_online": {
      const down = duration(what.down_ms);
      return withDetail(titles.nodeOnline(node), down && d.downFor(down));
    }
    case "node_degraded":
      return withDetail(titles.nodeDegraded(node), reasonText(what.reasons[0]));
    case "node_recovered": {
      const degraded = duration(what.degraded_ms);
      return withDetail(titles.nodeRecovered(node), degraded && d.degradedFor(degraded));
    }
    case "node_rebooted": {
      const up = what.up_s === null ? null : formatDuration(what.up_s);
      return withDetail(titles.nodeRebooted(node), up && d.upBefore(up));
    }
    case "flash_low":
      return withDetail(
        titles.flashLow(node),
        d.flash(formatBytes(what.available_bytes), formatBytes(what.total_bytes)),
      );
    case "flash_ok":
      return withDetail(
        titles.flashOk(node),
        d.flash(formatBytes(what.available_bytes), formatBytes(what.total_bytes)),
      );
    case "mesh_link_down":
      return { title: titles.meshDown(names.node(what.peer), node) };
    case "mesh_link_up": {
      const down = duration(what.down_ms);
      return withDetail(titles.meshUp(names.node(what.peer), node), down && d.downFor(down));
    }
    case "mesh_link_weak":
      return withDetail(titles.meshWeak(names.node(what.peer), node), d.quality(what.quality));
    case "mesh_link_recovered": {
      const weak = duration(what.weak_ms);
      return withDetail(
        titles.meshRecovered(names.node(what.peer), node),
        weak ? d.weakFor(what.quality, weak) : d.quality(what.quality),
      );
    }
    case "wan_down":
      return { title: titles.wanDown(node, what.device) };
    case "wan_up": {
      const down = duration(what.down_ms);
      return withDetail(titles.wanUp(node, what.device), down && d.downFor(down));
    }
    case "wan_address_changed":
      return withDetail(
        titles.wanAddress(node, what.interface),
        `${what.before.join(", ") || "—"} → ${what.after.join(", ") || "—"}`,
      );
    case "client_joined": {
      const place =
        what.medium === "wired"
          ? [d.wired, what.place]
          : [
              what.ssid,
              what.band === null ? null : strings.device.radios.band[what.band],
              what.place,
            ];
      return withDetail(titles.clientJoined(client, placed), place.filter(Boolean).join(" · "));
    }
    case "client_left": {
      const connected = duration(what.connected_ms);
      return withDetail(titles.clientLeft(client, placed), connected && d.connectedFor(connected));
    }
    case "client_roamed":
      return withDetail(
        titles.clientRoamed(client, names.node(what.from.node), names.node(what.to.node)),
        what.fast ? d.fast : `${what.from.iface} → ${what.to.iface}`,
      );
    case "new_client":
      return withDetail(
        titles.newClient(client, placed),
        [what.vendor, what.hostname].filter(Boolean).join(" · "),
      );
    case "log_pattern": {
      const meaning = names.pattern(what.pattern);
      const source = what.host ?? (event.node === null ? null : node);
      const title = meaning ?? titles.logPattern(what.pattern);
      return withDetail(
        source === null ? title : `${source}: ${title}`,
        what.program ? `${what.program}: ${what.text}` : what.text,
      );
    }
    case "job_failed":
      return withDetail(titles.jobFailed(d.jobKind[what.job_kind]), what.error);
    case "config_drift":
      return withDetail(
        what.removed
          ? titles.configDriftRemoved(node, what.package)
          : titles.configDrift(node, what.package),
        d.driftSince(what.revision),
      );
    case "sqm_rates_drifted":
      return withDetail(
        titles.sqmRatesDrifted,
        d.sqmRates(
          formatRate(what.download_kbps),
          formatRate(what.upload_kbps),
          formatRate(what.shaped_download_kbps),
          formatRate(what.shaped_upload_kbps),
        ),
      );
    case "config_drift_ended": {
      const how = d.driftEnd[what.how];
      const lasted = duration(what.drift_ms);
      return withDetail(
        titles.configDriftEnded(node, what.package),
        lasted ? d.driftFor(how, lasted) : how,
      );
    }
    case "channel_changed": {
      const band = strings.device.radios.band[what.band];
      return withDetail(
        titles.channelChanged(node, what.radio, what.from, what.to),
        what.radar
          ? d.radar
          : [band, what.width && `${what.width} MHz`].filter(Boolean).join(" · "),
      );
    }
    case "channel_congested":
      return withDetail(titles.channelCongested(node, what.radio), d.busy(what.busy_percent));
    case "channel_clear": {
      const lasted = duration(what.congested_ms);
      return withDetail(
        titles.channelClear(node, what.radio),
        lasted ? d.busyFor(what.busy_percent, lasted) : d.busy(what.busy_percent),
      );
    }
    case "wlan_scheduled":
      return { title: titles.wlanScheduled(what.ssid, what.on) };
    case "wlan_schedule_failed":
      return withDetail(titles.wlanScheduleFailed(node, what.phy), what.error);
    case "wlan_schedule_ok":
      return { title: titles.wlanScheduleOk(node, what.phy) };
    case "client_kicked": {
      const ban = what.ban_secs > 0 ? formatDuration(what.ban_secs) : null;
      return withDetail(
        titles.clientKicked(client),
        [what.by && d.by(what.by), ban && d.keptOff(ban)].filter(Boolean).join(" · "),
      );
    }
    case "client_blocked":
      return withDetail(titles.clientBlocked(client), what.by && d.by(what.by));
    case "client_unblocked":
      return withDetail(titles.clientUnblocked(client), what.by && d.by(what.by));
    case "client_bans_failed":
      return withDetail(titles.clientBansFailed(node), what.error);
    case "client_bans_ok":
      return { title: titles.clientBansOk(node) };
  }
}

/** Before and after an event, the logs page shows its entries: 5 minutes on each side. */
const AROUND_MS = 5 * 60_000;

/** The logs page around an event: its device's entries, or every source's. */
export function logsAround(event: EventRecord): string {
  const params = new URLSearchParams({
    from: String(event.at - AROUND_MS),
    to: String(event.last_at + AROUND_MS),
  });
  if (event.node !== null) params.set("nodes", String(event.node));
  return `/insights/logs?${params}`;
}

/** Whether `event` passes `filter`, as the store decides (`EventFilter::admits`). */
export function admits(filter: EventFilter, event: EventRecord): boolean {
  const rank = (level: EventLevel) => LEVELS.indexOf(level);
  return (
    (filter.categories.length === 0 || filter.categories.includes(event.category)) &&
    (filter.min === null || rank(event.level) <= rank(filter.min)) &&
    (filter.nodes.length === 0 || (event.node !== null && filter.nodes.includes(event.node))) &&
    (filter.clients.length === 0 ||
      (event.client !== null && filter.clients.includes(event.client))) &&
    (filter.from === null || event.at >= filter.from) &&
    (filter.to === null || event.at < filter.to)
  );
}

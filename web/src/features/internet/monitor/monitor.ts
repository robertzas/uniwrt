/**
 * The WAN quality monitor, pure (observability.md, Troubleshooting assistants; D-144): the
 * latest round as table rows, the health in words, and the outages its events describe.
 */
import type { EventRecord } from "../../../api/gen/EventRecord";
import type { WanFault } from "../../../api/gen/WanFault";
import type { WanMonitorSettings } from "../../../api/gen/WanMonitorSettings";
import type { WanMonitorView } from "../../../api/gen/WanMonitorView";
import type { WanProbe } from "../../../api/gen/WanProbe";
import type { WanProbeTarget } from "../../../api/gen/WanProbeTarget";
import { strings } from "../../../lib/strings";
import type { Tone } from "../../../ui/Badge";

const t = strings.internet.monitor;

/** One series of the latest round, as the table shows it. */
export interface ProbeRow {
  key: string;
  name: string;
  /** What it probes: `1.1.1.1`, `openwrt.org · 192.0.2.53`. */
  detail: string;
  kind: WanProbe["kind"] | undefined;
  /** The fastest, median and slowest answers (ms); `null` when none came. */
  min: number | null;
  median: number | null;
  max: number | null;
  /** The share lost, 0–100; `null` when nothing was sent. */
  loss: number | null;
  error: string | null;
}

/** The fastest, median and slowest of `values`, as the model's `ProbeResult::band`. */
export function band(values: readonly number[]): [number, number, number] | undefined {
  const sorted = [...values].sort((a, b) => a - b);
  const first = sorted[0];
  const last = sorted.at(-1);
  if (first === undefined || last === undefined) return undefined;
  const middle = Math.floor(sorted.length / 2);
  const median =
    sorted.length % 2 === 0
      ? ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2
      : (sorted[middle] ?? 0);
  return [first, median, last];
}

/** The target a series key (`<id>` or `<id>@<address>`) belongs to. */
export function targetOf(settings: WanMonitorSettings, key: string): WanProbeTarget | undefined {
  const id = key.split("@")[0];
  return settings.targets.find((target) => target.id === id);
}

/** The address a key names after `@` (one of the WAN's DNS servers). */
function addressOf(key: string): string | undefined {
  const at = key.indexOf("@");
  return at < 0 ? undefined : key.slice(at + 1);
}

/** A series' name: its target's, with the server's address for one of the WAN's. */
export function seriesName(settings: WanMonitorSettings, key: string): string {
  const target = targetOf(settings, key);
  const address = addressOf(key);
  if (!target) return key;
  return address === undefined ? target.name : `${target.name} · ${address}`;
}

function detailOf(probe: WanProbe | undefined, address: string | undefined, hop: string | null) {
  switch (probe?.kind) {
    case "icmp":
      return probe.host ?? hop ?? t.firstHopName;
    case "dns": {
      const via =
        probe.via.via === "server" ? probe.via.address : (address ?? t.via[probe.via.via]);
      return `${probe.name} · ${via}`;
    }
    case "http":
      return probe.url;
    default:
      return "";
  }
}

/** The latest round's series, in the order the gateway answered them. */
export function rowsOf(view: WanMonitorView): ProbeRow[] {
  const round = view.round;
  if (!round) return [];
  return round.results.map((result) => {
    const target = targetOf(view.settings, result.key);
    const found = band(result.rtt_ms);
    const lost = Math.max(0, result.sent - result.rtt_ms.length);
    return {
      key: result.key,
      name: seriesName(view.settings, result.key),
      detail: detailOf(target?.probe, addressOf(result.key), round.first_hop),
      kind: target?.probe.kind,
      min: found?.[0] ?? null,
      median: found?.[1] ?? null,
      max: found?.[2] ?? null,
      loss: result.sent > 0 ? (lost * 100) / result.sent : null,
      error: result.error,
    };
  });
}

/** The health as a badge: its tone and words. */
export function healthOf(view: WanMonitorView): { tone: Tone; label: string } {
  switch (view.health?.state) {
    case "up":
      return { tone: "ok", label: t.health.up };
    case "down":
      return { tone: "crit", label: t.health.down };
    default:
      return { tone: "neutral", label: t.health.unknown };
  }
}

/** Where the outage lies, while the internet is down. */
export function faultOf(view: WanMonitorView): WanFault | undefined {
  return view.health?.state === "down" ? view.health.fault : undefined;
}

/** A loss's tone: none is fine, all is critical. */
export function lossTone(loss: number | null): Tone {
  if (loss === null || loss >= 100) return "crit";
  return loss > 0 ? "warn" : "ok";
}

/** An outage: from `wan_outage` to `wan_restored`, `end` `null` while it lasts. */
export interface Outage {
  start: number;
  end: number | null;
  fault: WanFault;
  lastHop: string | null;
}

/**
 * The outages `events` (any order, others ignored) describe, newest first. A restore whose
 * outage began before the events starts at its `down_ms`.
 */
export function outagesOf(events: readonly EventRecord[]): Outage[] {
  const sorted = [...events].sort((a, b) => a.at - b.at);
  const out: Outage[] = [];
  let open: Outage | undefined;
  for (const event of sorted) {
    const what = event.what;
    if (what.kind === "wan_outage") {
      if (!open) {
        open = { start: event.at, end: null, fault: what.fault, lastHop: what.last_hop };
        out.push(open);
      }
    } else if (what.kind === "wan_restored") {
      if (open) {
        open.end = event.at;
        open = undefined;
      } else if (what.down_ms !== null) {
        out.push({
          start: event.at - what.down_ms,
          end: event.at,
          fault: what.fault,
          lastHop: null,
        });
      }
    }
  }
  return out.reverse();
}

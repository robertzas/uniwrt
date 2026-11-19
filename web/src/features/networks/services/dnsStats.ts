import type { Blocking } from "../../../api/gen/Blocking";
import type { DnsOutcome } from "../../../api/gen/DnsOutcome";
import type { DnsStats } from "../../../api/gen/DnsStats";
import { isDnsName } from "../../../lib/names";
import { strings } from "../../../lib/strings";
import type { Tone } from "../../../ui/Badge";
import { MAX_DOMAINS } from "./blocking";

const e = strings.networks.blocking.editor.errors;
export const HOUR = 3_600_000;

/** One bar of the chart: a span's queries, blocked ones included. */
export interface Bar {
  start: number;
  queries: number;
  blocked: number;
}

/**
 * The chart's bars over the stats' range: one per hour for 24 h, else one per local day, from the
 * span holding `since` to the one holding `now`; spans without queries are zeros.
 */
export function barsOf(stats: DnsStats, now: number): Bar[] {
  const starts: number[] = [];
  if (stats.range === "24h") {
    for (let hour = stats.since - (stats.since % HOUR); hour <= now; hour += HOUR) {
      starts.push(hour);
    }
  } else {
    const day = new Date(stats.since);
    day.setHours(0, 0, 0, 0);
    // Calendar steps, not 24 h ones: a day across a DST change is 23 or 25 hours long.
    while (day.getTime() <= now) {
      starts.push(day.getTime());
      day.setDate(day.getDate() + 1);
    }
  }
  const bars = starts.map((start) => ({ start, queries: 0, blocked: 0 }));
  if (bars.length === 0) return bars;
  let index = 0;
  for (const hour of stats.hours) {
    while (index + 1 < bars.length && (bars[index + 1]?.start ?? Infinity) <= hour.hour) index++;
    const bar = bars[index];
    if (!bar) continue;
    bar.queries += hour.queries;
    bar.blocked += hour.blocked;
  }
  return bars;
}

/** The tallest bar's queries, at least 1 (the chart's scale). */
export function peakOf(bars: readonly Bar[]): number {
  return bars.reduce((top, bar) => Math.max(top, bar.queries), 1);
}

const share = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });

/** The blocked share as text ("12.5 %"), or undefined without queries. */
export function blockedShare(queries: number, blocked: number): string | undefined {
  return queries > 0 ? `${share.format((100 * blocked) / queries)} %` : undefined;
}

/** How each outcome shows: blocked stands out, failures warn. */
export const OUTCOME_TONE: Record<DnsOutcome, Tone> = {
  forwarded: "neutral",
  cached: "info",
  local: "ok",
  blocked: "crit",
  failed: "warn",
};

export type Listed = "allowed" | "denied";

/**
 * Whether `blocking`'s own lists allow or block `domain`, itself or through a parent; the most
 * specific entry wins, as in the compiled file.
 */
export function listedOf(blocking: Blocking, domain: string): Listed | undefined {
  const name = domain.toLowerCase();
  const depth = (list: readonly string[]) =>
    list.reduce(
      (best, entry) =>
        (name === entry || name.endsWith(`.${entry}`)) && entry.length > best ? entry.length : best,
      -1,
    );
  const allow = depth(blocking.allow);
  const deny = depth(blocking.deny);
  if (allow < 0 && deny < 0) return undefined;
  return allow >= deny ? "allowed" : "denied";
}

/** Whether `domain` can go on the allow or deny list (the model checks the same). */
export function canList(domain: string): boolean {
  return domain.includes(".") && isDnsName(domain);
}

/** `blocking` with `domain` on `list`, and off the other one; or why it can't take it. */
export function withListed(
  blocking: Blocking,
  domain: string,
  list: "allow" | "deny",
): { blocking: Blocking } | { error: string } {
  const name = domain.toLowerCase();
  if (!canList(name)) return { error: e.domain(domain) };
  const other = list === "allow" ? "deny" : "allow";
  const entries = blocking[list].includes(name) ? blocking[list] : [...blocking[list], name];
  if (entries.length > MAX_DOMAINS) return { error: e.tooManyDomains };
  return {
    blocking: {
      ...blocking,
      [list]: entries,
      [other]: blocking[other].filter((entry) => entry !== name),
    },
  };
}

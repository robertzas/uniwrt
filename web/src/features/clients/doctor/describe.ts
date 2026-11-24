import type { DoctorFix } from "../../../api/gen/DoctorFix";
import type { DoctorIssue } from "../../../api/gen/DoctorIssue";
import type { DoctorProblem } from "../../../api/gen/DoctorProblem";
import type { DoctorSeverity } from "../../../api/gen/DoctorSeverity";
import type { TimelineEntry } from "../../../api/gen/TimelineEntry";
import { describeEvent, levelTones, type Names } from "../../../lib/events";
import { formatCount, formatDbm, formatPercent } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import type { Tone } from "../../../ui/Badge";

const t = strings.client.doctor;
const e = t.evidence;
const bands = strings.device.radios.band;

/** The spans the doctor looks over, in hours. */
export const DOCTOR_HOURS = [1, 24, 168] as const;
export type DoctorHours = (typeof DOCTOR_HOURS)[number];

/** How long the diagnosis stays current: the doctor reads stored data, so a minute is plenty. */
export const DOCTOR_POLL_MS = 60_000;

export const severityTones: Record<DoctorSeverity, Tone> = {
  critical: "crit",
  warning: "warn",
  info: "info",
};

/** The evidence of a problem in words. */
export function evidenceText(problem: DoctorProblem, nodeName: (id: number) => string): string {
  const join = (parts: (string | null | false)[]) =>
    parts.filter((part): part is string => Boolean(part)).join(", ");
  const share = (part: number, whole: number) =>
    formatPercent(whole === 0 ? 0 : (part * 100) / whole);
  switch (problem.kind) {
    case "weak_signal": {
      const where = [
        problem.node === null ? null : nodeName(problem.node),
        problem.band && bands[problem.band],
      ]
        .filter(Boolean)
        .join(" · ");
      return join([e.median(formatDbm(problem.signal_dbm)), where && e.at(where)]);
    }
    case "sticky":
      return join([
        e.sticky(formatDbm(problem.signal_dbm), problem.minutes),
        problem.node !== null && e.at(nodeName(problem.node)),
        problem.declined > 0 && e.declined(problem.declined),
      ]);
    case "auth_failures":
      return join([
        e.handshakes(problem.failures),
        problem.ssid !== null && e.on(problem.ssid),
        problem.connected ? e.joinedSince : e.notJoined,
      ]);
    case "dhcp_failures":
      return join([e.dhcp(problem.failures), e.dhcpReason[problem.reason]]);
    case "no_address":
      return e.noAddress(problem.minutes);
    case "dns_failures":
      return e.dnsFailed(
        formatCount(problem.failed),
        formatCount(problem.queries),
        share(problem.failed, problem.queries),
      );
    case "dns_blocked":
      return e.dnsBlocked(
        formatCount(problem.blocked),
        formatCount(problem.queries),
        share(problem.blocked, problem.queries),
      );
    case "stuck_on2g":
      return join([
        e.stuck(formatDbm(problem.signal_dbm)),
        problem.ssid !== null && e.on(problem.ssid),
      ]);
    case "fast_roaming":
      return join([
        e.on(problem.ssid),
        e.neverFast,
        problem.failures > 0 && e.ftFailures(problem.failures),
        problem.short_sessions > 0 && e.shortSessions(problem.short_sessions),
      ]);
    case "flapping":
      return e.flapping(problem.disconnects);
  }
}

/** A fix in words, and the page where it's made, if any. */
export interface FixText {
  text: string;
  link?: { href: string; label: string };
  /** The fix is reconnecting: an admin can kick the client from here. */
  reconnect?: true;
}

export function fixText(fix: DoctorFix, nodeName: (id: number) => string): FixText {
  const f = t.fix;
  const wifi = { href: "/wifi", label: f.open.wifi };
  const networks = { href: "/networks", label: f.open.networks };
  switch (fix.fix) {
    case "move_closer":
      return { text: fix.node === null ? f.moveCloser : f.moveCloserTo(nodeName(fix.node)) };
    case "add_access_point":
      return { text: f.addAccessPoint, link: { href: "/devices", label: f.open.devices } };
    case "turn_on_steering":
      return { text: f.turnOnSteering(fix.ssid), link: wifi };
    case "check_passphrase":
      return {
        text: fix.ssid === null ? f.checkPassphrase : f.checkPassphraseOf(fix.ssid),
        link: wifi,
      };
    case "widen_dhcp_pool":
      return { text: f.widenDhcpPool, link: networks };
    case "reconnect":
      return { text: f.reconnect, reconnect: true };
    case "check_dhcp_server":
      return { text: f.checkDhcpServer, link: networks };
    case "check_upstream_dns":
      return { text: f.checkUpstreamDns, link: networks };
    case "bypass_filtering":
      return { text: f.bypassFiltering, link: networks };
    case "turn_off_fast_roaming":
      return { text: f.turnOffFastRoaming(fix.ssid), link: wifi };
    case "update_client":
      return { text: f.updateClient };
  }
}

/** The chip's words: the worst issue, and how many more. */
export function chipText(issues: readonly DoctorIssue[]): string | undefined {
  const [worst] = issues;
  return worst && t.chip(t.problem[worst.problem.kind], issues.length - 1);
}

/** A timeline entry in words. */
export interface EntryText {
  title: string;
  detail?: string;
  tone: Tone;
  /** The node it happened at, when known. */
  node: number | null;
}

const DOMAINS_SHOWN = 5;
const TROUBLE_DHCP = new Set(["nak", "decline", "no_address"]);

export function entryText(entry: TimelineEntry, names: Names): EntryText {
  const tl = t.timeline;
  const detail = (...parts: (string | null | undefined)[]) => {
    const text = parts.filter(Boolean).join(" · ");
    return text ? { detail: text } : {};
  };
  switch (entry.kind) {
    case "event": {
      const described = describeEvent(entry.event, names);
      return {
        ...described,
        tone: levelTones[entry.event.level],
        node: entry.event.node,
      };
    }
    case "dhcp":
      return {
        title: tl.dhcp[entry.step],
        ...detail(entry.ip, entry.interface, entry.detail),
        tone: TROUBLE_DHCP.has(entry.step) ? "warn" : "neutral",
        node: entry.node,
      };
    case "wifi":
      return {
        title: tl.wifi[entry.step],
        ...detail(entry.interface, entry.detail),
        tone: "warn",
        node: entry.node,
      };
    case "log":
      return {
        title: entry.message,
        ...detail(entry.program, entry.node === null ? entry.host : null),
        tone: "neutral",
        node: entry.node,
      };
    case "dns": {
      const more = entry.domains.length - DOMAINS_SHOWN;
      const shown = entry.domains.slice(0, DOMAINS_SHOWN).join(", ");
      return {
        title: tl.dns(entry.failed),
        ...detail(more > 0 ? `${shown} ${tl.moreDomains(more)}` : shown),
        tone: "warn",
        node: null,
      };
    }
  }
}

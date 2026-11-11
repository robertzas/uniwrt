import type { Client } from "../../api/gen/Client";
import type { Issue } from "../../api/gen/Issue";
import type { Schedule } from "../../api/gen/Schedule";
import type { Span } from "../../api/gen/Span";
import type { Weekday } from "../../api/gen/Weekday";
import type { Wlan } from "../../api/gen/Wlan";
import { strings } from "../../lib/strings";
import { clock, WEEKDAYS } from "./draft";

const t = strings.wifi;

/** The clients online on a WLAN, by its SSID. */
export function clientsOn(wlan: Wlan, clients: readonly Client[]): number {
  return clients.filter((client) => client.online && client.connection?.ssid === wlan.ssid).length;
}

/** The document's warnings about a WLAN. */
export function warningsOf(issues: readonly Issue[], id: string): string[] {
  return issues
    .filter((issue) => issue.path === `wlans/${id}` || issue.path.startsWith(`wlans/${id}/`))
    .map((issue) => issue.message);
}

/** Which nodes a WLAN is on, in words. */
export function scopeText(wlan: Wlan, nodeName: (id: number) => string): string {
  const scope = wlan.nodes;
  if (scope.scope === "all") return t.scope.all;
  const names = scope.nodes.map(nodeName).join(", ");
  return scope.scope === "only" ? t.scope.only(names) : t.scope.except(names);
}

/** `Mon–Fri`, `Sat, Sun`, `Every day`: runs of days as ranges. */
export function daysText(days: readonly Weekday[]): string {
  const on = WEEKDAYS.map((day) => days.includes(day));
  if (on.every(Boolean)) return t.schedule.everyDay;
  const parts: string[] = [];
  for (let i = 0; i < 7; i++) {
    if (!on[i]) continue;
    let end = i;
    while (end + 1 < 7 && on[end + 1]) end++;
    const first = t.schedule.days[WEEKDAYS[i] as Weekday] ?? "";
    const last = t.schedule.days[WEEKDAYS[end] as Weekday] ?? "";
    parts.push(end === i ? first : end === i + 1 ? `${first}, ${last}` : `${first}–${last}`);
    i = end;
  }
  return parts.join(", ");
}

/** `Mon–Fri 07:00–22:00`. */
export function spanText(span: Span): string {
  return span.from === span.to
    ? `${daysText(span.days)} ${t.schedule.allDay}`
    : `${daysText(span.days)} ${clock(span.from)}–${clock(span.to)}`;
}

const INDEX: Record<string, number> = { Mon: 0, Tue: 1, Wed: 2, Thu: 3, Fri: 4, Sat: 5, Sun: 6 };

/** The weekday and minute it is in `zone` at `at`; `undefined` for a zone the browser lacks. */
export function localTime(zone: string, at: Date): { day: Weekday; minute: number } | undefined {
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      weekday: "short",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(at);
    const part = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
    const day = WEEKDAYS[INDEX[part("weekday")] ?? -1];
    const minute = Number(part("hour")) * 60 + Number(part("minute"));
    return day === undefined || Number.isNaN(minute) ? undefined : { day, minute };
  } catch {
    return undefined;
  }
}

/** Whether a schedule has the WLAN on at `minute` of `day` (as `Schedule::is_on` in the model). */
export function isOn(schedule: Schedule, day: Weekday, minute: number): boolean {
  const before = WEEKDAYS[(WEEKDAYS.indexOf(day) + 6) % 7] as Weekday;
  return schedule.spans.some((span) =>
    span.from < span.to
      ? span.days.includes(day) && minute >= span.from && minute < span.to
      : (span.days.includes(day) && minute >= span.from) ||
        (span.days.includes(before) && minute < span.to),
  );
}

/** Whether the schedule has the WLAN on now in the site's zone; `undefined` when unknown. */
export function onNow(schedule: Schedule, zone: string, at: Date): boolean | undefined {
  const now = localTime(zone, at);
  return now && isOn(schedule, now.day, now.minute);
}

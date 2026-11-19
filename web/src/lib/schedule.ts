/**
 * Weekly schedules in the site's time zone (wifi.md, WLANs; network.md, Scheduled restarts and
 * LEDs schedule; D-083, D-138): the days of the week, spans and times as typed and as stored,
 * in words, and what they mean at a moment (as the model's `Schedule::is_on`).
 */
import type { RebootTime } from "../api/gen/RebootTime";
import type { Schedule } from "../api/gen/Schedule";
import type { Span } from "../api/gen/Span";
import type { Weekday } from "../api/gen/Weekday";
import { clockText } from "./format";
import { strings } from "./strings";

const t = strings.schedule;

export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** Minutes after midnight as `HH:MM`. */
export const clock = clockText;

/** `HH:MM` as minutes after midnight. */
export function minutesOf(text: string): number | undefined {
  const match = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(text.trim());
  return match ? Number(match[1]) * 60 + Number(match[2]) : undefined;
}

/** A span of a schedule as typed: `from` and `to` are `HH:MM`. */
export interface SpanDraft {
  days: Weekday[];
  from: string;
  to: string;
}

/** The span a new schedule starts with: every day, 07:00 to 22:00. */
export const newSpan = (): SpanDraft => ({ days: [...WEEKDAYS], from: "07:00", to: "22:00" });

/** A stored schedule's spans as typed; none: one new span. */
export function spanDrafts(
  schedule: Schedule | null,
  fresh: () => SpanDraft = newSpan,
): SpanDraft[] {
  return schedule
    ? schedule.spans.map((span) => ({
        days: [...span.days],
        from: clock(span.from),
        to: clock(span.to),
      }))
    : [fresh()];
}

/** The days in the week's order. */
export const ordered = (days: readonly Weekday[]): Weekday[] =>
  WEEKDAYS.filter((day) => days.includes(day));

/** The spans the drafts describe, or what is wrong with them. */
export function spansOf(drafts: readonly SpanDraft[]): Span[] | string {
  if (drafts.length === 0) return t.errors.noSpans;
  const spans: Span[] = [];
  for (const draft of drafts) {
    const from = minutesOf(draft.from);
    const to = minutesOf(draft.to);
    if (from === undefined || to === undefined) return t.errors.time;
    if (draft.days.length === 0) return t.errors.noDays;
    spans.push({ days: ordered(draft.days), from, to });
  }
  return spans;
}

/** A restart time as typed: `at` is `HH:MM`. */
export interface TimeDraft {
  days: Weekday[];
  at: string;
}

/** A stored restart time as typed; none: Sundays at 04:00. */
export function timeDraft(time: RebootTime | null): TimeDraft {
  return time ? { days: [...time.days], at: clock(time.at) } : { days: ["sun"], at: "04:00" };
}

/** The restart time the draft describes, or what is wrong with it. */
export function timeOf(draft: TimeDraft): RebootTime | string {
  const at = minutesOf(draft.at);
  if (at === undefined) return t.errors.time;
  if (draft.days.length === 0) return t.errors.noDay;
  return { days: ordered(draft.days), at };
}

/** `Mon–Fri`, `Sat, Sun`, `Every day`: runs of days as ranges. */
export function daysText(days: readonly Weekday[]): string {
  const on = WEEKDAYS.map((day) => days.includes(day));
  if (on.every(Boolean)) return t.everyDay;
  const parts: string[] = [];
  for (let i = 0; i < 7; i++) {
    if (!on[i]) continue;
    let end = i;
    while (end + 1 < 7 && on[end + 1]) end++;
    const first = t.days[WEEKDAYS[i] as Weekday] ?? "";
    const last = t.days[WEEKDAYS[end] as Weekday] ?? "";
    parts.push(end === i ? first : end === i + 1 ? `${first}, ${last}` : `${first}–${last}`);
    i = end;
  }
  return parts.join(", ");
}

/** `Mon–Fri 07:00–22:00`. */
export function spanText(span: Span): string {
  return span.from === span.to
    ? `${daysText(span.days)} ${t.allDay}`
    : `${daysText(span.days)} ${clock(span.from)}–${clock(span.to)}`;
}

/** `Sun 04:00`. */
export function timeText(time: RebootTime): string {
  return `${daysText(time.days)} ${clock(time.at)}`;
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

/** Whether a schedule is on at `minute` of `day` (as `Schedule::is_on` in the model). */
export function isOn(schedule: Schedule, day: Weekday, minute: number): boolean {
  const before = WEEKDAYS[(WEEKDAYS.indexOf(day) + 6) % 7] as Weekday;
  return schedule.spans.some((span) =>
    span.from < span.to
      ? span.days.includes(day) && minute >= span.from && minute < span.to
      : (span.days.includes(day) && minute >= span.from) ||
        (span.days.includes(before) && minute < span.to),
  );
}

/** Whether the schedule is on now in the site's zone; `undefined` when unknown. */
export function onNow(schedule: Schedule, zone: string, at: Date): boolean | undefined {
  const now = localTime(zone, at);
  return now && isOn(schedule, now.day, now.minute);
}

/** The wall clock of `zone` at `at`, as if it were UTC (ms); throws for an unknown zone. */
function wallClock(zone: string, at: number): number {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: zone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date(at));
  const n = (type: string) => Number(parts.find((p) => p.type === type)?.value);
  return Date.UTC(n("year"), n("month") - 1, n("day"), n("hour"), n("minute"), n("second"));
}

/**
 * The moment `zone`'s clocks show `wall` (a wall clock as UTC ms). A time the move to summer
 * time skips comes the gap's length later (02:30 as 03:30).
 */
function instantOf(zone: string, wall: number): number {
  const guess = wall - (wallClock(zone, wall) - wall);
  return wall - (wallClock(zone, guess) - guess);
}

/**
 * The next time after `now` that `time` comes in `zone`: when the controller restarts the nodes
 * that follow it (D-138). `undefined` without days or for a zone the browser lacks.
 */
export function nextTime(time: RebootTime, zone: string, now: Date): Date | undefined {
  if (time.days.length === 0) return undefined;
  try {
    const wall = wallClock(zone, now.getTime());
    const today = new Date(wall);
    for (let k = 0; k <= 7; k++) {
      const date = Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() + k);
      const day = WEEKDAYS[(new Date(date).getUTCDay() + 6) % 7] as Weekday;
      if (!time.days.includes(day)) continue;
      const at = instantOf(zone, date + time.at * 60_000);
      if (at > now.getTime()) return new Date(at);
    }
  } catch {
    return undefined;
  }
  return undefined;
}

/** A moment as the site's clocks show it: `Sun, Nov 22, 04:00`. */
export function zonedText(at: Date, zone: string): string {
  return new Intl.DateTimeFormat(undefined, {
    timeZone: zone,
    weekday: "short",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).format(at);
}

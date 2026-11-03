/** Maintenance windows and snoozes, pure: the form checked as the server checks it. */
import type { MaintenanceWindow } from "../../api/gen/MaintenanceWindow";
import type { NewWindow } from "../../api/gen/NewWindow";
import { strings } from "../../lib/strings";

const e = strings.alerts.maintenance.errors;
const DAY = 86_400_000;

/** How long a snooze lasts: an hour, four, a day. */
export const SNOOZES = [3_600_000, 4 * 3_600_000, DAY] as const;

/** A window from now for `ms`, for one device or every device. */
export function snooze(ms: number, node: number | null, now = Date.now()): NewWindow {
  return {
    starts_at: now,
    ends_at: now + ms,
    nodes: node === null ? [] : [node],
    note: strings.alerts.maintenance.snoozeNote,
  };
}

/** On now or to come, as of `now`. */
export function isOn(window: MaintenanceWindow, now = Date.now()): boolean {
  return window.starts_at <= now && now < window.ends_at;
}

/** Windows on now first (ending soonest), then those to come (starting soonest). */
export function sortWindows(
  windows: readonly MaintenanceWindow[],
  now = Date.now(),
): MaintenanceWindow[] {
  return windows
    .filter((window) => window.ends_at > now)
    .sort(
      (a, b) =>
        Number(isOn(b, now)) - Number(isOn(a, now)) ||
        (isOn(a, now) ? a.ends_at - b.ends_at : a.starts_at - b.starts_at) ||
        a.id - b.id,
    );
}

/** A time as a `datetime-local` input holds it, in local time to the minute. */
export function toLocalInput(ms: number): string {
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}`;
}

/** A `datetime-local` input's value as a time, or undefined when it holds none. */
export function fromLocalInput(value: string): number | undefined {
  if (value === "") return undefined;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? ms : undefined;
}

export interface WindowDraft {
  starts: string;
  ends: string;
  /** Every device, or only `nodes`. */
  every: boolean;
  nodes: number[];
  note: string;
}

/** A fresh form: from now for an hour, every device. */
export function emptyDraft(now = Date.now()): WindowDraft {
  const start = Math.ceil(now / 60_000) * 60_000;
  return {
    starts: toLocalInput(start),
    ends: toLocalInput(start + 3_600_000),
    every: true,
    nodes: [],
    note: "",
  };
}

export type WindowErrors = Partial<Record<"starts" | "ends" | "nodes", string>>;

/** The body to send, or what is wrong with the form (`create` in `alerts/maintenance.rs`). */
export function checkWindow(
  draft: WindowDraft,
  now = Date.now(),
): { window: NewWindow } | { errors: WindowErrors } {
  const starts = fromLocalInput(draft.starts);
  const ends = fromLocalInput(draft.ends);
  const errors: WindowErrors = {};
  if (starts === undefined) errors.starts = e.date;
  if (ends === undefined) errors.ends = e.date;
  if (starts !== undefined && ends !== undefined) {
    if (ends <= starts) errors.ends = e.order;
    else if (ends <= now) errors.ends = e.past;
    else if (ends - starts > 30 * DAY) errors.ends = e.long;
    if (starts - now > 365 * DAY) errors.starts = e.far;
  }
  if (!draft.every && draft.nodes.length === 0) errors.nodes = e.nodes;
  if (!draft.every && draft.nodes.length > 32) errors.nodes = strings.alerts.editor.errors.tooMany;
  if (starts === undefined || ends === undefined || Object.keys(errors).length > 0) {
    return { errors };
  }
  return {
    window: {
      starts_at: starts,
      ends_at: ends,
      nodes: draft.every ? [] : [...draft.nodes].sort((a, b) => a - b),
      note: draft.note.trim(),
    },
  };
}

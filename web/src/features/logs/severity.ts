/** Syslog severities and facilities as the logs page shows them. */
import type { Severity } from "../../api/gen/Severity";
import type { Tone } from "../../ui/Badge";

/** Most severe first, so a severity's index is its syslog code. */
export const SEVERITIES: readonly Severity[] = [
  "emergency",
  "alert",
  "critical",
  "error",
  "warning",
  "notice",
  "info",
  "debug",
];

export function isSeverity(value: string | undefined): value is Severity {
  return value !== undefined && (SEVERITIES as readonly string[]).includes(value);
}

/** The syslog code: 0 for `emergency` … 7 for `debug`. */
export function severityCode(severity: Severity): number {
  return SEVERITIES.indexOf(severity);
}

/** The four bands the page colors severities in: errors and worse, warnings, info, debug. */
export type Level = "error" | "warning" | "info" | "debug";

export const LEVELS: readonly Level[] = ["error", "warning", "info", "debug"];

export function levelOf(severity: Severity): Level {
  const code = severityCode(severity);
  if (code <= 3) return "error";
  if (code === 4) return "warning";
  if (code <= 6) return "info";
  return "debug";
}

export const LEVEL_TONES: Record<Level, Tone> = {
  error: "crit",
  warning: "warn",
  info: "info",
  debug: "neutral",
};

/** Background classes for a level's marks (bars, row edges): status colors, 3:1, never text. */
export const LEVEL_FILL: Record<Level, string> = {
  error: "bg-crit",
  warning: "bg-warn",
  info: "bg-info",
  debug: "bg-fg-subtle/50",
};

/** SVG fills for the same, as CSS variables. */
export const LEVEL_SVG_FILL: Record<Level, string> = {
  error: "var(--color-crit)",
  warning: "var(--color-warn)",
  info: "var(--color-info)",
  debug: "color-mix(in oklab, var(--color-fg-subtle) 50%, transparent)",
};

/** A histogram bucket's counts (one per severity code) summed per level. */
export function countsByLevel(counts: readonly number[]): Record<Level, number> {
  const sums: Record<Level, number> = { error: 0, warning: 0, info: 0, debug: 0 };
  counts.forEach((count, code) => {
    const severity = SEVERITIES[code];
    if (severity !== undefined) sums[levelOf(severity)] += count;
  });
  return sums;
}

const FACILITIES = [
  "kern",
  "user",
  "mail",
  "daemon",
  "auth",
  "syslog",
  "lpr",
  "news",
  "uucp",
  "cron",
  "authpriv",
  "ftp",
  "ntp",
  "security",
  "console",
  "cron2",
];

/** A facility's conventional name (RFC 5424): `daemon`, `local3`…, or its code. */
export function facilityName(code: number): string {
  if (code >= 16 && code <= 23) return `local${code - 16}`;
  return FACILITIES[code] ?? String(code);
}

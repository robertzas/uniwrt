/** Notifiers in words: where they send, which alerts they hear of, how their deliveries went. */
import type { DeliveryStatus } from "../../../api/gen/DeliveryStatus";
import type { NodeId } from "../../../api/gen/NodeId";
import type { NotifierFilter } from "../../../api/gen/NotifierFilter";
import type { NotifierView } from "../../../api/gen/NotifierView";
import { formatDuration } from "../../../lib/format";
import { strings } from "../../../lib/strings";

const t = strings.settings.notifications;

/** A reminder interval in words: "Every 4 hours", "Off". */
export function remindName(seconds: number): string {
  if (seconds <= 0) return t.filter.remindOff;
  return (
    t.filter.remindEvery[seconds as keyof typeof t.filter.remindEvery] ??
    t.filter.every(formatDuration(seconds))
  );
}

/** Where a notifier sends: `ntfy.sh/uniwrt-abc`, `Discord · https://discord.com`, an address. */
export function targetOf(view: NotifierView): string {
  const spec = view.spec;
  if (spec.kind === "ntfy") {
    const host = spec.server.replace(/^https?:\/\//, "");
    return `${host}/${spec.topic}`;
  }
  if (spec.kind === "webhook") {
    const format = t.formats[spec.format].label;
    return view.secrets.url === null ? format : `${format} · ${view.secrets.url}`;
  }
  const first = spec.to[0] ?? "";
  return spec.to.length > 1 ? t.toMany(first, spec.to.length - 1) : first;
}

/** Which alerts a notifier hears of: `Warning and above · Every device · Resolutions · …`. */
export function filterOf(filter: NotifierFilter, nodeName: (id: NodeId) => string | undefined) {
  const f = t.filter;
  const level =
    filter.min === "critical"
      ? f.criticalOnly
      : filter.min === "info"
        ? f.everyLevel
        : f.andAbove(strings.events.levelName[filter.min]);
  const names = filter.nodes.map((id) => nodeName(id) ?? `#${id}`);
  const devices =
    names.length === 0
      ? f.everyDevice
      : names.length <= 3
        ? names.join(", ")
        : f.devicesMore(names.slice(0, 2).join(", "), names.length - 2);
  const remind =
    filter.remind_s === null
      ? f.noReminders
      : f.reminded(remindName(filter.remind_s).toLowerCase());
  return [level, devices, filter.resolved ? f.withResolutions : f.noResolutions, remind].join(
    " · ",
  );
}

export type Delivery =
  | { state: "none" }
  | { state: "ok"; at: number }
  | { state: "failed"; at: number; why: string };

/** The latest delivery: none yet, a success, or a failure with why. */
export function deliveryOf(status: DeliveryStatus): Delivery {
  const ok = status.last_ok_at;
  const failed = status.last_error_at;
  if (failed !== null && (ok === null || failed > ok)) {
    return { state: "failed", at: failed, why: status.last_error ?? t.status.unknown };
  }
  return ok === null ? { state: "none" } : { state: "ok", at: ok };
}

/** How long ago `at` was, at minute steps under an hour: `5m ago`, `just now`. */
export function ago(at: number, now: number): string {
  const s = Math.max(0, (now - at) / 1000);
  if (s < 60) return t.status.justNow;
  return t.status.ago(formatDuration(s < 3600 ? Math.floor(s / 60) * 60 : s));
}

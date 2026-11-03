/** What the alerts page's parts share: names, tiles, fields and actions. */
import type { JSX } from "@solidjs/web";
import { createMemo } from "solid-js";
import type { Names } from "../../lib/alerts";
import { api, errorText } from "../../lib/api";
import { createClientsView } from "../../lib/clientList";
import { clientName } from "../../lib/clients";
import type { Live } from "../../lib/live";
import { strings } from "../../lib/strings";
import { showToast } from "../../ui/Toast";

/** The tile behind an alert's icon, tinted by its level. */
export const TILES = {
  crit: "border-crit/30 bg-crit/10 text-crit",
  warn: "border-warn/30 bg-warn/10 text-warn",
  info: "border-info/30 bg-info/10 text-info",
  neutral: "border-edge bg-raised text-fg-subtle",
} as const;

/** A small outlined link that looks like a button, for row actions. */
export const ACTION =
  "inline-flex h-7 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised";

export const full = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "medium",
});
export const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });

/** Devices by their names and clients by theirs, live, while the calling component lives. */
export function createNames(live: Live): Names {
  const clients = createClientsView(live);
  const nodes = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));
  const macs = createMemo(() => new Map(clients.clients.map((c) => [c.mac, clientName(c)])));
  return { node: (id) => nodes().get(id), client: (mac) => macs().get(mac) };
}

/** Acknowledges an alert; the `alerts` topic brings the change. A failure shows a toast. */
export function acknowledge(id: number): Promise<void> {
  return api.alerts
    .acknowledge(id)
    .then(() => undefined)
    .catch((error: unknown) => {
      showToast({
        title: strings.alerts.acknowledgeFailed,
        description: errorText(error),
        tone: "crit",
      });
    });
}

/** A term and its value in a details list. */
export function Field(props: { label: string; children: JSX.Element }) {
  return (
    <>
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class="min-w-0 text-fg">{props.children}</dd>
    </>
  );
}

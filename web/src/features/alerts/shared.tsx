/** What the alerts page's parts share: names, tiles, fields, actions and error text. */
import type { JSX } from "@solidjs/web";
import { createMemo } from "solid-js";
import type { Names } from "../../lib/alerts";
import { api } from "../../lib/api";
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

/** A native select in the toolbar's style. */
export const SELECT =
  "h-9 w-full cursor-pointer rounded-control border border-edge bg-canvas pr-7 pl-2.5 text-fg text-sm focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-60";

/** A text or number input in the forms' style. */
export const INPUT =
  "h-9 w-full rounded-control border bg-canvas px-2.5 text-fg text-sm outline-none transition-[border-color,box-shadow] placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30 disabled:opacity-60";

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

/** The message of a failed call, for a toast. */
export function errorText(error: unknown): string | undefined {
  return error instanceof Error ? error.message : undefined;
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

/** A labelled control in a form: the label above, a hint or an error below. */
export function FormField(props: {
  label: string;
  for?: string;
  hint?: string | undefined;
  error?: string | undefined;
  children: JSX.Element;
  class?: string;
}) {
  return (
    <div class={["flex min-w-0 flex-col gap-1.5", props.class]}>
      <label for={props.for} class="font-medium text-fg text-sm">
        {props.label}
      </label>
      {props.children}
      <p
        class={["text-xs", props.error ? "text-crit" : "text-fg-subtle"]}
        hidden={!props.error && !props.hint}
      >
        {props.error ?? props.hint}
      </p>
    </div>
  );
}

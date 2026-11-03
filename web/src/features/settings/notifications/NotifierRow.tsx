import { BellRing, Mail, Pencil, Trash2, Webhook } from "lucide";
import { Match, Show, Switch } from "solid-js";
import type { NodeId } from "../../../api/gen/NodeId";
import type { NotifierView } from "../../../api/gen/NotifierView";
import { formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge, StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Icon } from "../../../ui/Icon";
import type { Kind } from "./draft";
import { ago, deliveryOf, filterOf, targetOf } from "./words";

const t = strings.settings.notifications;

export const KIND_ICONS: Record<Kind, typeof Mail> = {
  ntfy: BellRing,
  webhook: Webhook,
  email: Mail,
};

/** A notifier in the list: its kind, name, where it sends, its filter and its last delivery. */
export function NotifierRow(props: {
  notifier: NotifierView;
  admin: boolean;
  busy: boolean;
  now: number;
  nodeName: (id: NodeId) => string | undefined;
  onToggle: (enabled: boolean) => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const n = () => props.notifier;
  const delivery = () => deliveryOf(n().status);

  return (
    <li
      id={`notifier-${n().id}`}
      class="flex items-start gap-3 border-edge/60 border-b px-4 py-3 last:border-b-0"
    >
      <div class="mt-0.5 w-9 shrink-0">
        <Show
          when={props.admin}
          fallback={
            <span class={["text-xs", n().enabled ? "text-fg" : "text-fg-subtle"]}>
              {n().enabled ? t.on : t.off}
            </span>
          }
        >
          <Checkbox
            label={n().enabled ? t.disable(n().name) : t.enable(n().name)}
            hideLabel
            checked={n().enabled}
            disabled={props.busy}
            onChange={(enabled) => props.onToggle(enabled)}
          />
        </Show>
      </div>
      <div
        class={[
          "grid size-8 shrink-0 place-items-center rounded-control border border-edge bg-raised text-fg-muted",
          { "opacity-60": !n().enabled },
        ]}
      >
        <Icon icon={KIND_ICONS[n().spec.kind]} class="size-4" />
      </div>
      <div class={["flex min-w-0 flex-1 flex-col gap-1", { "opacity-60": !n().enabled }]}>
        <div class="flex flex-wrap items-center gap-2">
          <span class="font-medium text-fg text-sm">{n().name}</span>
          <Badge>{t.kinds[n().spec.kind]}</Badge>
        </div>
        <p class="truncate font-mono text-fg-muted text-xs">{targetOf(n())}</p>
        <p class="text-fg-subtle text-xs">{filterOf(n().filter, props.nodeName)}</p>
        <p class="flex min-w-0 items-start gap-1.5 text-xs">
          <Switch>
            <Match when={delivery().state === "none"}>
              <span class="text-fg-subtle">{t.status.none}</span>
            </Match>
            <Match when={delivery().state === "ok"}>
              <StatusDot tone="ok" class="mt-1" />
              <span class="text-fg-muted" title={formatDateTime(n().status.last_ok_at ?? 0)}>
                {t.status.delivered(ago(n().status.last_ok_at ?? 0, props.now))}
              </span>
            </Match>
            <Match when={delivery().state === "failed"}>
              <StatusDot tone="crit" class="mt-1" />
              <span
                class="min-w-0 break-words text-fg"
                title={formatDateTime(n().status.last_error_at ?? 0)}
              >
                {t.status.failed(ago(n().status.last_error_at ?? 0, props.now))}{" "}
                <span class="text-fg-muted">{n().status.last_error}</span>
              </span>
            </Match>
          </Switch>
        </p>
      </div>
      <Show when={props.admin}>
        <div class="flex shrink-0 items-center gap-1">
          <Button
            size="sm"
            variant="ghost"
            icon={Pencil}
            aria-label={`${t.edit}: ${n().name}`}
            onClick={() => props.onEdit()}
          >
            <span class="max-sm:hidden">{t.edit}</span>
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon={Trash2}
            aria-label={`${t.remove}: ${n().name}`}
            disabled={props.busy}
            onClick={() => props.onRemove()}
          />
        </div>
      </Show>
    </li>
  );
}

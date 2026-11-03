import { CircleCheck } from "lucide";
import { createMemo, For, Show } from "solid-js";
import { byAttention, type Names, unattended } from "../../lib/alerts";
import type { Live } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Card } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { AlertRow } from "./AlertRow";
import { SnoozeMenu } from "./SnoozeMenu";

const t = strings.alerts.active;

/**
 * The alerts firing, live from the app's `alerts` store: what wants attention first, then the
 * acknowledged and the silenced, each most severe first.
 */
export function ActiveTab(props: {
  live: Live;
  names: Names;
  now: number;
  admin: boolean;
  expanded: ReadonlySet<number>;
  onToggle: (id: number) => void;
}) {
  const alerts = createMemo(() => byAttention(props.live.alerts.active));
  const pending = createMemo(() => unattended(props.live.alerts.active).length);
  return (
    <Card class="overflow-hidden" aria-label={t.label}>
      <div class="flex flex-wrap items-center gap-3 border-edge border-b px-4 py-2.5">
        <p role="status" class="flex items-center gap-2 text-sm">
          <span class="font-medium text-fg tabular-nums">{t.count(alerts().length)}</span>
          <Show when={pending() > 0}>
            <span class="text-fg-subtle tabular-nums">· {t.attention(pending())}</span>
          </Show>
        </p>
        <Show when={props.admin}>
          <div class="ml-auto">
            <SnoozeMenu label={strings.alerts.maintenance.snooze} />
          </div>
        </Show>
      </div>
      <Show
        when={props.live.alerts.ready}
        fallback={
          <p role="status" class="py-20 text-center text-fg-subtle text-sm">
            {t.loading}…
          </p>
        }
      >
        <Show
          when={alerts().length > 0}
          fallback={
            <div class="py-6">
              <EmptyState icon={CircleCheck} title={t.emptyTitle} description={t.empty} />
            </div>
          }
        >
          <ul>
            <For each={alerts()}>
              {(alert) => (
                <AlertRow
                  alert={alert}
                  names={props.names}
                  now={props.now}
                  admin={props.admin}
                  markFiring={false}
                  expanded={props.expanded.has(alert.id)}
                  onToggle={() => props.onToggle(alert.id)}
                />
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </Card>
  );
}

import { For, Show, useContext } from "solid-js";
import type { LeaseFacts } from "../../api/gen/LeaseFacts";
import { describeEvent, type Names } from "../../lib/events";
import { formatDateTime } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { createSteering, outcomeCounts, outcomeOf, outcomeTones } from "../../lib/steering";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";

const t = strings.device.steering;
const DAY_MS = 86_400_000;
/** Events listed; the rest are on the events page. */
const SHOWN = 8;

/**
 * The clients this access point steered in the last 24 hours (wifi.md, Steering & balancing:
 * Timeline UI): what it came to, counted, then the newest, each linking to its client. Shown
 * only when there is some.
 */
export function SteeringCard(props: { node: number; leases: readonly LeaseFacts[] }) {
  const live = useContext(LiveContext);
  const steering = createSteering(() => ({ nodes: [props.node], clients: [], spanMs: DAY_MS }));
  const names: Names = {
    node: (id) => live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`,
    client: (mac) => props.leases.find((lease) => lease.mac === mac)?.hostname ?? mac,
    pattern: () => undefined,
  };
  const summary = () =>
    outcomeCounts(steering())
      .map(([outcome, n]) => strings.events.steerOutcome[outcome](n))
      .join(" · ");
  const more = () => steering().length - SHOWN;

  return (
    <Show when={steering().length > 0}>
      <Card aria-label={t.title}>
        <CardHeader title={t.title} description={`${t.description} · ${summary()}`} />
        <ul class="flex flex-col gap-1 px-4 pb-4 text-sm">
          <For each={steering().slice(0, SHOWN)}>
            {(event) => {
              const described = describeEvent(event, names);
              return (
                <li class="flex items-baseline gap-3">
                  <span
                    aria-hidden="true"
                    class={[
                      "size-2 shrink-0 self-center rounded-full",
                      outcomeTones[outcomeOf[event.what.action]],
                    ]}
                  />
                  <span class="min-w-36 shrink-0 whitespace-nowrap text-fg-subtle tabular-nums">
                    {formatDateTime(event.at)}
                  </span>
                  <span class="min-w-0">
                    <Show when={event.client} fallback={described.title}>
                      {(mac) => (
                        <a
                          href={`/clients/${encodeURIComponent(mac())}`}
                          class="hover:text-accent hover:underline"
                        >
                          {described.title}
                        </a>
                      )}
                    </Show>
                    <Show when={described.detail}>
                      <span class="text-fg-muted"> · {described.detail}</span>
                    </Show>
                  </span>
                </li>
              );
            }}
          </For>
          <Show when={more() > 0}>
            <li>
              <a
                href={`/insights/events?nodes=${props.node}&categories=client`}
                class="text-accent hover:underline"
              >
                {t.more(more())}
              </a>
            </li>
          </Show>
        </ul>
      </Card>
    </Show>
  );
}

import { ArrowDown, ArrowUp } from "lucide";
import { createMemo, For, Show } from "solid-js";
import type { Client } from "../../api/gen/Client";
import { clientName, kindIcons, upstreamName } from "../../lib/clients";
import { formatBitrate } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { topClients } from "./health";

const t = strings.dashboard.top;
const COUNT = 5;

/**
 * The clients moving the most traffic now (from their access points' counters), each with a bar
 * of its download and upload against the busiest one's.
 */
export function TopClients(props: {
  clients: readonly Client[];
  nodeName: (id: number) => string;
}) {
  const top = createMemo(() => topClients(props.clients, COUNT));
  const rate = (client: Client) => ({
    down: client.connection?.traffic?.down_bps ?? 0,
    up: client.connection?.traffic?.up_bps ?? 0,
  });
  const busiest = () => {
    const first = top()[0];
    if (!first) return 1;
    const { down, up } = rate(first);
    return Math.max(1, down + up);
  };

  return (
    <Card class="flex min-w-0 flex-col" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <a href="/clients" class="font-medium text-accent hover:underline">
            {t.all}
          </a>
        }
      />
      <Show
        when={top().length > 0}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.empty}</p>}
      >
        <ul class="flex flex-col px-2 pb-2">
          <For each={top()}>
            {(client) => {
              const now = () => rate(client);
              const share = (bps: number) => `${((100 * bps) / busiest()).toFixed(1)}%`;
              return (
                <li>
                  <a
                    href={`/clients/${client.mac}`}
                    class="flex min-w-0 items-center gap-3 rounded-control px-2 py-1.5 hover:bg-raised"
                  >
                    <span class="grid size-8 shrink-0 place-items-center rounded-lg bg-raised text-fg-muted">
                      <Icon icon={kindIcons[client.kind]} class="size-4" />
                    </span>
                    <span class="flex min-w-0 flex-1 flex-col gap-1">
                      <span class="flex min-w-0 items-baseline gap-2">
                        <span class="truncate font-medium text-sm">{clientName(client)}</span>
                        <Show when={client.connection}>
                          {(connection) => (
                            <span class="truncate text-fg-subtle text-xs">
                              {upstreamName(connection(), props.nodeName)}
                            </span>
                          )}
                        </Show>
                      </span>
                      <span
                        aria-hidden="true"
                        class="flex h-1.5 overflow-hidden rounded-full bg-raised"
                      >
                        <span class="bg-ok" style={{ width: share(now().down) }} />
                        <span class="bg-info" style={{ width: share(now().up) }} />
                      </span>
                    </span>
                    <span class="sr-only">
                      {t.rates(formatBitrate(now().down), formatBitrate(now().up))}
                    </span>
                    <span
                      aria-hidden="true"
                      class="flex shrink-0 flex-col items-end text-xs tabular-nums"
                    >
                      <span class="inline-flex items-center gap-0.5">
                        <Icon icon={ArrowDown} class="size-3 text-ok" />
                        {formatBitrate(now().down)}
                      </span>
                      <span class="inline-flex items-center gap-0.5 text-fg-subtle">
                        <Icon icon={ArrowUp} class="size-3 text-info" />
                        {formatBitrate(now().up)}
                      </span>
                    </span>
                  </a>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
    </Card>
  );
}

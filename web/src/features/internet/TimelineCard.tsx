import { useQuery } from "@tanstack/solid-query";
import { createMemo, For, Show } from "solid-js";
import { api } from "../../lib/api";
import { describeEvent, type Names } from "../../lib/events";
import { formatDateTime, formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { stretches } from "./uplink";

const t = strings.internet.timeline;
const WEEK_MS = 7 * 24 * 3600 * 1000;

/**
 * The WAN status timeline (wan.md, Failover; D-076): the last 7 days as a strip, the WAN down
 * in red and the traffic on the backup line in amber, then the events themselves.
 */
export function TimelineCard(props: { nodeName: (id: number) => string; now: number }) {
  const from = () => props.now - WEEK_MS;
  const events = useQuery(() => ({
    queryKey: ["wan-events", Math.floor(props.now / 60_000)],
    queryFn: ({ signal }) =>
      api.events.page(
        { categories: ["wan"], min: null, nodes: [], clients: [], from: from(), to: null },
        null,
        200,
        signal,
      ),
  }));
  const list = () => events.data?.events ?? [];
  const parts = createMemo(() => stretches(list(), from(), props.now));
  const names: Names = {
    node: (id) => props.nodeName(id),
    client: (mac) => mac,
    pattern: () => undefined,
  };
  const share = (ms: number) => `${(ms / WEEK_MS) * 100}%`;
  const quiet = () => {
    const down = parts().filter((part) => part.kind === "down");
    const since = down.length === 0 ? from() : (down.at(-1)?.end ?? from());
    return formatDuration((props.now - since) / 1000);
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <div
          class="relative h-3 overflow-hidden rounded-full bg-ok/25"
          role="img"
          aria-label={t.upFor(quiet())}
        >
          <For each={parts()}>
            {(part) => (
              <div
                class={[
                  "absolute inset-y-0 min-w-0.5",
                  part.kind === "down" ? "bg-crit" : "bg-warn",
                ]}
                style={{ left: share(part.start - from()), width: share(part.end - part.start) }}
              />
            )}
          </For>
        </div>
        <div class="flex justify-between text-fg-subtle text-xs">
          <span>{formatDateTime(from())}</span>
          <span>{t.upFor(quiet())}</span>
        </div>
        <Show when={events.isSuccess && list().length === 0}>
          <p class="text-fg-muted text-sm">{t.none}</p>
        </Show>
        <ul class="flex flex-col gap-1 text-sm">
          <For each={list().slice(0, 8)}>
            {(event) => {
              const described = describeEvent(event, names);
              return (
                <li class="flex gap-3">
                  <span class="w-36 shrink-0 text-fg-subtle tabular-nums">
                    {formatDateTime(event.at)}
                  </span>
                  <span class="min-w-0">
                    {described.title}
                    <Show when={described.detail}>
                      <span class="text-fg-muted"> · {described.detail}</span>
                    </Show>
                  </span>
                </li>
              );
            }}
          </For>
        </ul>
      </div>
    </Card>
  );
}

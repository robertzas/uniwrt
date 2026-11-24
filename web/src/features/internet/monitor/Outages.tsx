import { useQuery } from "@tanstack/solid-query";
import { createMemo, For, Show } from "solid-js";
import { api } from "../../../lib/api";
import { formatDateTime, formatDuration } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { outagesOf } from "./monitor";

const t = strings.internet.monitor.outages;
const SPAN_MS = 30 * 24 * 3600 * 1000;
const SHOWN = 20;

/** The outages the monitor found in the last 30 days, newest first (`wan_outage` → `wan_restored`). */
export function Outages(props: { now: number }) {
  const events = useQuery(() => ({
    queryKey: ["wan-outages", Math.floor(props.now / 60_000)],
    queryFn: ({ signal }) =>
      api.events.page(
        {
          categories: ["wan"],
          min: null,
          nodes: [],
          clients: [],
          from: props.now - SPAN_MS,
          to: null,
        },
        null,
        500,
        signal,
      ),
  }));
  const list = createMemo(() => outagesOf(events.data?.events ?? []));
  return (
    <section aria-label={t.title} class="flex flex-col gap-2">
      <div>
        <h3 class="font-medium text-sm">{t.title}</h3>
        <p class="text-fg-subtle text-xs">{t.description}</p>
      </div>
      <Show when={events.isSuccess && list().length === 0}>
        <p class="text-fg-muted text-sm">{t.none}</p>
      </Show>
      <ul class="flex flex-col gap-1 text-sm">
        <For each={list().slice(0, SHOWN)}>
          {(outage) => (
            <li class="flex flex-wrap items-baseline gap-x-3">
              <span class="w-36 shrink-0 text-fg-subtle tabular-nums">
                {formatDateTime(outage.start)}
              </span>
              <span class="min-w-0 flex-1">
                {t.fault[outage.fault]}
                <Show when={outage.lastHop}>
                  {(hop) => <span class="text-fg-muted"> · {t.lastHop(hop())}</span>}
                </Show>
              </span>
              <Show when={outage.end} fallback={<Badge tone="crit">{t.ongoing}</Badge>}>
                {(end) => (
                  <span class="text-fg-muted tabular-nums">
                    {t.lasted(formatDuration((end() - outage.start) / 1000))}
                  </span>
                )}
              </Show>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}

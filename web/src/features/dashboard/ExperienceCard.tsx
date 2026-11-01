import { createMemo, For, Show } from "solid-js";
import type { Client } from "../../api/gen/Client";
import { scoresByNode } from "../../lib/clientList";
import { scoreTone } from "../../lib/clients";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { scoreSpread, wifiHealth } from "./health";

const t = strings.dashboard.experience;
const RADIUS = 42;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

const strokes = { ok: "text-ok", warn: "text-warn", crit: "text-crit", neutral: "text-fg-subtle" };
const fills = { ok: "bg-ok", warn: "bg-warn", crit: "bg-crit" } as const;

/** The site's Wi-Fi experience score as a ring, how the clients' scores spread, the weakest AP. */
export function ExperienceCard(props: {
  clients: readonly Client[];
  nodeName: (id: number) => string;
}) {
  const site = createMemo(() => wifiHealth(props.clients));
  const spread = createMemo(() => scoreSpread(props.clients));
  const worst = createMemo(() => scoresByNode(props.clients)[0]);
  const score = () => site().score ?? 0;
  const total = () => spread().ok + spread().warn + spread().crit;

  return (
    <Card class="flex min-w-0 flex-col" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <a href="/clients" class="font-medium text-accent hover:underline">
            {strings.dashboard.top.all}
          </a>
        }
      />
      <Show
        when={site().score !== null}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.empty}</p>}
      >
        <div class="flex flex-wrap items-center gap-5 px-4 pb-4">
          <svg
            viewBox="0 0 100 100"
            role="img"
            aria-label={t.score(score())}
            class="size-28 shrink-0 -rotate-90"
          >
            <circle
              cx="50"
              cy="50"
              r={RADIUS}
              fill="none"
              stroke="currentColor"
              stroke-width="9"
              class="text-raised"
            />
            <circle
              cx="50"
              cy="50"
              r={RADIUS}
              fill="none"
              stroke="currentColor"
              stroke-width="9"
              stroke-linecap="round"
              stroke-dasharray={`${(CIRCUMFERENCE * score()) / 100} ${CIRCUMFERENCE}`}
              class={["transition-[stroke-dasharray] duration-500", strokes[scoreTone(score())]]}
            />
            <text
              x="50"
              y="50"
              transform="rotate(90 50 50)"
              text-anchor="middle"
              dominant-baseline="central"
              class="fill-fg font-semibold text-[1.6rem] tabular-nums"
            >
              {score()}
            </text>
          </svg>
          <div class="flex min-w-40 flex-1 flex-col gap-3">
            <div class="flex h-2 overflow-hidden rounded-full bg-raised" aria-hidden="true">
              <For each={["ok", "warn", "crit"] as const}>
                {(tone) => (
                  <span
                    class={fills[tone]}
                    style={{ width: `${(100 * spread()[tone]) / Math.max(1, total())}%` }}
                  />
                )}
              </For>
            </div>
            <ul class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
              <For each={["ok", "warn", "crit"] as const}>
                {(tone) => (
                  <li class="flex items-center gap-1.5">
                    <Badge tone={tone}>{t.spread[tone]}</Badge>
                    <span class="font-semibold tabular-nums">{spread()[tone]}</span>
                  </li>
                )}
              </For>
            </ul>
            <Show when={worst()}>
              {(entry) => (
                <div class="flex items-center justify-between gap-2 border-edge border-t pt-2 text-sm">
                  <span class="min-w-0">
                    <span class="block text-fg-subtle text-xs">{t.worst}</span>
                    <span class="block truncate font-medium">{props.nodeName(entry().node)}</span>
                  </span>
                  <Badge tone={scoreTone(entry().score)}>{entry().score}</Badge>
                </div>
              )}
            </Show>
          </div>
        </div>
      </Show>
    </Card>
  );
}

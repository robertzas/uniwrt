import {
  ArrowRightLeft,
  Cog,
  FileCog,
  FilePen,
  Network,
  Power,
  Server,
  ShieldCheck,
  ShieldHalf,
  TriangleAlert,
  Wifi,
} from "lucide";
import { For, Show } from "solid-js";
import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { Icon, type IconNode } from "../../ui/Icon";
import { type ImpactKind, impacts, longestTimer } from "./plan";

const t = strings.changes.impact;

const ICONS: Record<ImpactKind, IconNode> = {
  network: Network,
  wireless: Wifi,
  dhcp: Server,
  firewall: ShieldHalf,
  system: Cog,
  other: FileCog,
  service: Power,
  address: ArrowRightLeft,
  drift: FilePen,
  warnings: TriangleAlert,
};

/** The impact summary: what the apply interrupts, starts and stops, and that it rolls back. */
export function ImpactCard(props: { plan: ApplyPlan }) {
  const list = () => impacts(props.plan);
  return (
    <Show when={list().length > 0}>
      <Card aria-label={t.title}>
        <CardHeader title={t.title} />
        <ul class="flex flex-col gap-2 px-4 pb-4 text-sm">
          <For each={list()}>
            {(impact) => (
              <li class="flex items-start gap-2.5">
                <Icon
                  icon={ICONS[impact.kind]}
                  class={[
                    "mt-0.5 size-4 shrink-0",
                    impact.disruptive || impact.kind === "warnings" ? "text-warn" : "text-info",
                  ]}
                />
                <span class="text-fg">{impact.text}</span>
              </li>
            )}
          </For>
          <Show when={longestTimer(props.plan)}>
            {(seconds) => (
              <li class="mt-1 flex items-start gap-2.5 border-edge/60 border-t pt-3">
                <Icon icon={ShieldCheck} class="mt-0.5 size-4 shrink-0 text-ok" />
                <span class="text-fg-muted">{t.rollback(formatDuration(seconds()))}</span>
              </li>
            )}
          </Show>
        </ul>
      </Card>
    </Show>
  );
}

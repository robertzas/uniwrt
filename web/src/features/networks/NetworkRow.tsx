import { TriangleAlert } from "lucide";
import { For, Show } from "solid-js";
import type { Network } from "../../api/gen/Network";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import type { NetworkUse } from "./summary";

const t = strings.networks;

const TONES: Record<Network["purpose"], Tone> = {
  default: "info",
  iot: "warn",
  guest: "ok",
  mgmt: "neutral",
  custom: "neutral",
};

/** One network in the list: name, purpose and VLAN; subnet and gateway; pool use and clients. */
export function NetworkRow(props: {
  network: Network;
  use: NetworkUse;
  warnings: readonly string[];
  admin: boolean;
  onEdit: () => void;
}) {
  const n = () => props.network;
  const percent = () => {
    const pool = props.use.pool;
    return pool ? Math.min(100, Math.round((pool.leased / pool.size) * 100)) : 0;
  };
  const flags = (): string[] => [
    ...(n().isolation ? [t.flags.isolation] : []),
    ...(n().mdns ? [t.flags.mdns] : []),
    ...(n().igmp_snooping ? [t.flags.igmp] : []),
  ];
  return (
    <li class="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-start" aria-label={n().name}>
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <h3 class="font-medium text-sm">{n().name}</h3>
          <Badge tone={TONES[n().purpose]}>{t.purpose[n().purpose]}</Badge>
          <span class="rounded-control border border-edge px-1.5 py-0.5 font-mono text-[11px] text-fg-muted">
            {n().vlan === null ? t.untagged : t.vlan(n().vlan ?? 0)}
          </span>
        </div>
        <p class="mt-1 flex flex-wrap gap-x-3 text-fg-muted text-xs">
          <Show when={props.use.subnet} fallback={<span>{t.noIpv4}</span>}>
            <span class="font-mono">{props.use.subnet}</span>
            <span>{t.gatewayAt(props.use.gateway ?? "")}</span>
          </Show>
          <span>{t.ipv6[n().ipv6]}</span>
          <Show when={props.use.reservations > 0}>
            <span>{t.reservations(props.use.reservations)}</span>
          </Show>
          <For each={flags()}>{(flag) => <span>{flag}</span>}</For>
        </p>
        <For each={props.warnings}>
          {(warning) => (
            <p class="mt-1.5 flex items-start gap-1.5 text-warn text-xs">
              <Icon icon={TriangleAlert} class="mt-px size-3.5 shrink-0" />
              {warning}
            </p>
          )}
        </For>
      </div>
      <div class="flex w-full shrink-0 flex-col gap-1.5 sm:w-56">
        <Show
          when={props.use.pool}
          fallback={<span class="text-fg-subtle text-xs">{props.use.subnet ? t.noDhcp : ""}</span>}
        >
          {(pool) => (
            <>
              <div class="flex justify-between gap-2 text-xs">
                <span class="text-fg-muted">{t.pool(pool().leased, pool().size)}</span>
                <span class="text-fg-subtle tabular-nums">{percent()}%</span>
              </div>
              <div
                class="h-1.5 overflow-hidden rounded-full bg-raised"
                aria-hidden="true"
                title={t.poolRange(pool().start, pool().end)}
              >
                <div
                  class={[
                    "h-full rounded-full transition-[width]",
                    percent() >= 90 ? "bg-crit" : percent() >= 75 ? "bg-warn" : "bg-accent",
                  ]}
                  style={{ width: `${percent()}%` }}
                />
              </div>
            </>
          )}
        </Show>
        <span class="text-fg-muted text-xs">{t.clients(props.use.clients)}</span>
      </div>
      <Show when={props.admin}>
        <Button size="sm" onClick={() => props.onEdit()} aria-label={`${t.edit} ${n().name}`}>
          {t.edit}
        </Button>
      </Show>
    </li>
  );
}

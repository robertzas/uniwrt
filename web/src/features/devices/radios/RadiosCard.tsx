import { useQuery } from "@tanstack/solid-query";
import { Pencil, Pin } from "lucide";
import { createSignal, For, Show, useContext } from "solid-js";
import type { Band } from "../../../api/gen/Band";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { RadioFacts } from "../../../api/gen/RadioFacts";
import type { WifiIfaceFacts } from "../../../api/gen/WifiIfaceFacts";
import { api } from "../../../lib/api";
import { formatDbm, formatPercent, NONE } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { SavedRevision } from "../../../ui/SavedRevision";
import { RadioDialog } from "./RadioDialog";
import { overrideOf, type Pins, pinsOf, policyOf } from "./radios";

const r = strings.device.radios;

const bandDot: Record<Band, string> = {
  "2g": "bg-band-2g",
  "5g": "bg-band-5g",
  "6g": "bg-band-6g",
  "60g": "bg-fg-subtle",
};

/** The interfaces' names that stations of `radio` are associated with. */
function ifaceNames(radio: RadioFacts): Set<string> {
  return new Set(radio.ifaces.flatMap((iface) => (iface.name ? [iface.name] : [])));
}

/** Clients (not mesh peers) per radio. */
export function clientsOf(facts: NodeFacts, radio: RadioFacts): number {
  const names = ifaceNames(radio);
  return facts.stations.filter((s) => s.peer_link === null && names.has(s.iface)).length;
}

function ifaceLabel(iface: WifiIfaceFacts): string {
  if (iface.mode === "mesh") return r.mesh(iface.mesh_id ?? NONE);
  if (iface.mode === "sta") return r.uplink(iface.ssid ?? NONE);
  return iface.ssid ?? iface.section ?? NONE;
}

/** What the radio is fixed to, in words; `null` when nothing is. */
function pinsText(pins: Pins): string | null {
  if (pins.off) return r.turnedOff;
  const parts = [
    pins.channel !== null && r.fixedChannel(pins.channel),
    pins.width !== null && r.width(pins.width),
    pins.power !== null && r.fixedPower(pins.power),
  ].filter((part): part is string => typeof part === "string");
  return parts.length > 0 ? r.fixed(parts) : null;
}

/**
 * Every radio: band, channel and width, power, noise, how busy the channel is, and its networks;
 * what is fixed by hand, which admins edit (D-155).
 */
export function RadiosCard(props: { node: Node; facts: NodeFacts }) {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const [editing, setEditing] = createSignal<RadioFacts>();
  const [saved, setSaved] = createSignal<number>();
  /** The radio being edited, with the site it edits. */
  const open = () => {
    const radio = editing();
    const config = site.data?.config;
    return radio && config ? { radio, config } : undefined;
  };
  const pins = (radio: RadioFacts) => {
    const config = site.data?.config;
    if (!config) return null;
    return pinsText(
      pinsOf(overrideOf(config, props.node.id, radio.name), policyOf(config, radio.band)),
    );
  };
  return (
    <Card class="min-w-0" aria-label={r.title}>
      <CardHeader title={r.title} />
      <Show
        when={props.facts.radios.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{r.empty}</p>}
      >
        <ul class="divide-y divide-edge">
          <For each={props.facts.radios}>
            {(radio) => (
              <RadioRow
                radio={radio}
                clients={clientsOf(props.facts, radio)}
                pins={pins(radio)}
                onEdit={admin() && site.data ? () => setEditing(radio) : undefined}
              />
            )}
          </For>
        </ul>
      </Show>
      <Show when={saved()}>
        {(revision) => (
          <div class="px-4 pb-4">
            <SavedRevision revision={revision()} />
          </div>
        )}
      </Show>
      <Show when={open()}>
        {(open) => (
          <RadioDialog
            node={props.node}
            facts={props.facts}
            radio={open().radio}
            config={open().config}
            onClose={() => setEditing(undefined)}
            onSaved={(revision) => {
              setEditing(undefined);
              setSaved(revision);
            }}
          />
        )}
      </Show>
    </Card>
  );
}

function RadioRow(props: {
  radio: RadioFacts;
  clients: number;
  pins: string | null;
  onEdit: (() => void) | undefined;
}) {
  const radio = () => props.radio;
  const on = () => radio().up && !radio().disabled;
  /** Channel 36 · 80 MHz · Wi-Fi 6, leaving out what is unknown. */
  const tuning = () => {
    const { channel, width, standard } = radio();
    return [
      channel && r.channel(channel),
      width && r.width(width),
      standard && r.standard[standard],
    ]
      .filter(Boolean)
      .join(" · ");
  };
  return (
    <li class="px-4 py-3">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span class="flex items-center gap-2 font-medium text-sm">
          <span class={["size-2 rounded-full", bandDot[radio().band]]} aria-hidden="true" />
          {r.band[radio().band]}
        </span>
        <span class="font-mono text-fg-subtle text-xs">{radio().name}</span>
        <Show
          when={on()}
          fallback={<Badge tone="neutral">{radio().disabled ? r.disabled : r.off}</Badge>}
        >
          <span class="text-fg-muted text-sm">{tuning()}</span>
          <Show when={radio().restart_pending}>
            <Badge tone="info">{r.restartPending}</Badge>
          </Show>
        </Show>
        <Show when={props.pins}>{(text) => <Badge icon={Pin}>{text()}</Badge>}</Show>
        <Show when={props.onEdit}>
          {(open) => (
            <Button
              size="sm"
              variant="ghost"
              class="ml-auto"
              aria-label={r.editLabel(radio().name)}
              onClick={open()}
            >
              <Icon icon={Pencil} class="size-3.5" />
              {r.edit}
            </Button>
          )}
        </Show>
      </div>
      <Show when={on()}>
        <dl class="mt-2.5 grid grid-cols-2 gap-x-4 gap-y-2 text-xs sm:grid-cols-4">
          <Metric label={r.clients} value={String(props.clients)} />
          <Metric
            label={r.txPower}
            value={radio().tx_power_dbm == null ? NONE : `${radio().tx_power_dbm} dBm`}
          />
          <Metric label={r.noise} value={formatDbm(radio().noise_dbm)} />
          <div class="min-w-0">
            <dt class="text-fg-subtle">{r.busy}</dt>
            <dd class="mt-0.5 flex items-center gap-2 font-medium tabular-nums">
              {formatPercent(radio().channel_busy_percent)}
              <Show when={radio().channel_busy_percent}>{(busy) => <Meter value={busy()} />}</Show>
            </dd>
          </div>
        </dl>
        <div class="mt-2.5 flex flex-wrap gap-1.5">
          <For each={radio().ifaces}>
            {(iface) => (
              <Badge tone={iface.up ? "ok" : "neutral"}>
                <span class="max-w-48 truncate">{ifaceLabel(iface)}</span>
              </Badge>
            )}
          </For>
        </div>
      </Show>
    </li>
  );
}

function Metric(props: { label: string; value: string }) {
  return (
    <div class="min-w-0">
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class="mt-0.5 font-medium tabular-nums">{props.value}</dd>
    </div>
  );
}

/** A small bar for a share; amber from 50 %, red from 75 %, as airtime gets scarce. */
function Meter(props: { value: number }) {
  return (
    <span class="h-1.5 w-12 overflow-hidden rounded-full bg-raised" aria-hidden="true">
      <span
        class={[
          "block h-full rounded-full",
          props.value >= 75 ? "bg-crit" : props.value >= 50 ? "bg-warn" : "bg-ok",
        ]}
        style={{ width: `${Math.min(100, props.value)}%` }}
      />
    </span>
  );
}

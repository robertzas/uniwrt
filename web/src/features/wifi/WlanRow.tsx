import { Clock, QrCode, TriangleAlert } from "lucide";
import { For, Show } from "solid-js";
import type { Band } from "../../api/gen/Band";
import type { Wlan } from "../../api/gen/Wlan";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { needsPassphrase } from "./draft";
import { spanText } from "./summary";

const t = strings.wifi;

const TONES: Record<Wlan["security"], Tone> = {
  wpa2: "neutral",
  wpa2_wpa3: "info",
  wpa3: "ok",
  owe: "ok",
  owe_transition: "info",
  open: "warn",
};

/** The band colors every chart and legend uses (ui.md, Visual language). */
export const BAND_DOT: Record<Band, string> = {
  "2g": "bg-band-2g",
  "5g": "bg-band-5g",
  "6g": "bg-band-6g",
  "60g": "bg-fg-subtle",
};

/** A band's name behind a dot of its color. */
export function BandChip(props: { band: Band }) {
  return (
    <span class="inline-flex items-center gap-1.5 rounded-control border border-edge px-1.5 py-0.5 text-[11px] text-fg-muted">
      <span class={["size-2 rounded-full", BAND_DOT[props.band]]} aria-hidden="true" />
      {strings.device.radios.band[props.band]}
    </span>
  );
}

/**
 * One WLAN in the list: its name, security and bands; its network, devices, schedule and
 * options; the clients on it now.
 */
export function WlanRow(props: {
  wlan: Wlan;
  networkName: string;
  scope: string;
  clients: number;
  /** Whether its schedule has it on now; `undefined` without a schedule or a known zone. */
  on: boolean | undefined;
  warnings: readonly string[];
  admin: boolean;
  onEdit: () => void;
  onJoin: () => void;
}) {
  const w = () => props.wlan;
  return (
    <li
      class={["flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-start"]}
      aria-label={w().ssid}
    >
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <h3 class={["font-medium text-sm", { "text-fg-muted": !w().enabled }]}>{w().ssid}</h3>
          <Show when={!w().enabled}>
            <Badge tone="neutral">{t.off}</Badge>
          </Show>
          <Badge tone={TONES[w().security]}>{t.security[w().security]}</Badge>
          <For each={w().bands}>{(band) => <BandChip band={band} />}</For>
        </div>
        <p class="mt-1.5 flex flex-wrap gap-x-3 text-fg-muted text-xs">
          <span>{t.network(props.networkName)}</span>
          <span>{props.scope}</span>
          <Show when={w().hidden}>
            <span>{t.hidden}</span>
          </Show>
          <Show when={w().isolation}>
            <span>{t.isolation}</span>
          </Show>
          <Show when={w().max_clients}>{(max) => <span>{t.maxClients(max())}</span>}</Show>
        </p>
        <Show when={w().schedule}>
          {(schedule) => (
            <p class="mt-1.5 flex flex-wrap items-center gap-x-2 text-fg-muted text-xs">
              <Icon icon={Clock} class="size-3.5 shrink-0" />
              <span>{schedule().spans.map(spanText).join(" · ")}</span>
              <Show when={w().enabled && props.on !== undefined}>
                <Badge tone={props.on ? "ok" : "neutral"}>
                  {props.on ? t.schedule.on : t.schedule.off}
                </Badge>
              </Show>
            </p>
          )}
        </Show>
        <For each={props.warnings}>
          {(warning) => (
            <p class="mt-1.5 flex items-start gap-1.5 text-warn text-xs">
              <Icon icon={TriangleAlert} class="mt-px size-3.5 shrink-0" />
              {warning}
            </p>
          )}
        </For>
      </div>
      <span class="shrink-0 text-fg-muted text-xs sm:w-32 sm:pt-1 sm:text-right">
        {t.clients(props.clients)}
      </span>
      <div class="flex shrink-0 gap-2">
        {/* A passphrase is shown to admins only; a network without one is anyone's to join. */}
        <Show when={props.admin || !needsPassphrase(w().security)}>
          <Button
            size="sm"
            icon={QrCode}
            onClick={() => props.onJoin()}
            aria-label={`${t.join} ${w().ssid}`}
          />
        </Show>
        <Show when={props.admin}>
          <Button size="sm" onClick={() => props.onEdit()} aria-label={`${t.edit} ${w().ssid}`}>
            {t.edit}
          </Button>
        </Show>
      </div>
    </li>
  );
}

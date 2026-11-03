import { For, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import type { Neighbor } from "../../api/gen/Neighbor";
import type { RadioReport } from "../../api/gen/RadioReport";
import type { RadioScan } from "../../api/gen/RadioScan";
import { formatDbm, formatPercent, NONE } from "../../lib/format";
import { busyTone, CROWDING_TONE, crowding, relative } from "../../lib/rf";
import { strings } from "../../lib/strings";
import { Badge, StatusDot } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { networkName } from "./Spectrum";

const t = strings.rf.radios;
const SHOWN = 3;

/** When a radio was scanned and what comes next, in words. */
export function scanLine(scan: RadioScan | undefined, now: number): string {
  if (!scan) return strings.rf.scanning.never;
  const s = strings.rf.scanning;
  const last = scan.last ? s.last(relative(scan.last.at, now)) : s.never;
  const state = scan.state;
  if (state === "scheduled" && scan.next_at !== null) {
    return `${last} · ${s.next(relative(scan.next_at, now))}`;
  }
  return `${last} · ${s.state[state]}`;
}

/** The band's radios: channel and width, how busy and crowded it is, who shares it, scans. */
export function RadioCards(props: {
  band: BandReport;
  scans: readonly RadioScan[];
  nodeName: (id: number) => string;
  admin: boolean;
  now: number;
  onScan: (node: number, radio: string) => void;
  onSelect: (bssid: string | undefined) => void;
}) {
  const networks = () => new Map(props.band.networks.map((n) => [n.bss.bssid, n]));
  const scanOf = (radio: RadioReport) =>
    props.scans.find((s) => s.node === radio.node && s.radio === radio.radio);
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <ul class="grid gap-3 px-4 pb-4 md:grid-cols-2 xl:grid-cols-3">
        <For each={props.band.radios}>
          {(radio) => (
            <RadioCard
              radio={radio}
              scan={scanOf(radio)}
              networks={networks()}
              nodeName={props.nodeName}
              admin={props.admin}
              now={props.now}
              onScan={() => props.onScan(radio.node, radio.radio)}
              onSelect={props.onSelect}
            />
          )}
        </For>
      </ul>
    </Card>
  );
}

function RadioCard(props: {
  radio: RadioReport;
  scan: RadioScan | undefined;
  networks: Map<string, Neighbor>;
  nodeName: (id: number) => string;
  admin: boolean;
  now: number;
  onScan: () => void;
  onSelect: (bssid: string | undefined) => void;
}) {
  const r = () => props.radio;
  const crowd = () => crowding(r().interference);
  const tuning = () =>
    [
      r().channel === null ? null : t.channel(r().channel as number),
      r().width === null ? null : t.width(r().width as number),
      r().tx_power_dbm === null ? null : `${r().tx_power_dbm} dBm`,
    ]
      .filter(Boolean)
      .join(" · ");
  const scanned = () => props.scan?.last != null;
  const busy = () => props.scan?.state === "scanning" || props.scan?.state === "queued";
  return (
    <li class="flex flex-col gap-3 rounded-control border border-edge p-3">
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <a
            href={`/devices/${r().node}`}
            class="block truncate font-medium hover:text-accent hover:underline"
          >
            {props.nodeName(r().node)}
          </a>
          <p class="truncate text-fg-muted text-xs">
            {r().radio}
            <Show when={tuning()}>{(text) => ` · ${text()}`}</Show>
          </p>
        </div>
        <Show when={r().up} fallback={<Badge tone="neutral">{t.down}</Badge>}>
          <Show when={scanned()}>
            <Badge tone={CROWDING_TONE[crowd()]}>{t.crowding[crowd()]}</Badge>
          </Show>
        </Show>
      </div>
      <dl class="grid grid-cols-3 gap-2 text-xs">
        <div>
          <dt class="text-fg-subtle">{t.busy}</dt>
          <dd class="flex items-center gap-1.5 font-medium font-mono tabular-nums">
            <StatusDot tone={busyTone(r().busy_percent)} />
            {formatPercent(r().busy_percent)}
          </dd>
        </div>
        <div>
          <dt class="text-fg-subtle">{t.noise}</dt>
          <dd class="font-medium font-mono tabular-nums">{formatDbm(r().noise_dbm)}</dd>
        </div>
        <div>
          <dt class="text-fg-subtle">{strings.rf.networks.impact}</dt>
          <dd class="font-medium font-mono tabular-nums">
            {scanned() ? r().interference.toFixed(2) : NONE}
          </dd>
        </div>
      </dl>
      <div class="flex flex-col gap-1">
        <p class="font-medium text-fg-subtle text-xs uppercase tracking-wide">{t.onChannel}</p>
        <Show when={scanned()} fallback={<p class="text-fg-subtle text-xs">{t.notScanned}</p>}>
          <Show
            when={r().on_channel.length > 0}
            fallback={<p class="text-fg-muted text-xs">{t.nobody}</p>}
          >
            <ul class="flex flex-col gap-0.5 text-xs">
              <For each={r().on_channel.slice(0, SHOWN)}>
                {(o) => {
                  const n = () => props.networks.get(o.bssid);
                  return (
                    <li
                      class="flex items-center justify-between gap-2"
                      onMouseEnter={() => props.onSelect(o.bssid)}
                      onMouseLeave={() => props.onSelect(undefined)}
                    >
                      <span class="truncate">
                        {n() ? networkName(n() as Neighbor, props.nodeName) : o.bssid}
                        <span class="text-fg-subtle">
                          {` · ${o.co_channel ? t.coChannel : t.overlaps(Math.round(o.overlap * 100))}`}
                        </span>
                      </span>
                      <span class="shrink-0 font-mono text-fg-muted tabular-nums">
                        {formatDbm(o.signal_dbm)}
                      </span>
                    </li>
                  );
                }}
              </For>
              <Show when={r().on_channel.length > SHOWN}>
                <li class="text-fg-subtle">{t.more(r().on_channel.length - SHOWN)}</li>
              </Show>
            </ul>
          </Show>
        </Show>
      </div>
      <div class="mt-auto flex items-center justify-between gap-2 border-edge border-t pt-2">
        <p class="min-w-0 truncate text-fg-subtle text-xs" title={props.scan?.error ?? undefined}>
          {scanLine(props.scan, props.now)}
        </p>
        <Show when={props.admin}>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy() || !r().up}
            aria-label={t.scanOne(`${props.nodeName(r().node)} ${r().radio}`)}
            onClick={props.onScan}
          >
            {t.scan}
          </Button>
        </Show>
      </div>
    </li>
  );
}

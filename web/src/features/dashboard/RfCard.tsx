import { For, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import { formatPercent } from "../../lib/format";
import type { Live } from "../../lib/live";
import { busyTone, createRfView, crowding } from "../../lib/rf";
import { strings } from "../../lib/strings";
import { Badge, StatusDot } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";

const t = strings.rf.dashboard;

/** One band at a glance: its radios and channels, the busiest, the neighbors and lookalikes. */
export function bandSummary(band: BandReport) {
  const up = band.radios.filter((r) => r.up);
  const busiest = up.reduce<(typeof up)[number] | undefined>(
    (worst, r) => ((r.busy_percent ?? -1) > (worst?.busy_percent ?? -1) ? r : worst),
    undefined,
  );
  const channels = [...new Set(up.flatMap((r) => (r.channel === null ? [] : [r.channel])))].sort(
    (a, b) => a - b,
  );
  return {
    radios: band.radios.length,
    channels,
    busiest,
    neighbors: band.networks.filter((n) => n.kind === "foreign").length,
    lookalikes: band.networks.filter((n) => n.kind === "lookalike").length,
    crowded: up.filter((r) => crowding(r.interference) === "crowded").length,
  };
}

/** The dashboard's RF summary: per band, how busy and crowded the air is (wifi.md). */
export function RfCard(props: { live: Live; nodeName: (id: number) => string }) {
  const view = createRfView(props.live);
  const bands = () => view()?.report.bands ?? [];
  return (
    <Card class="min-w-0" aria-label={t.title}>
      <CardHeader
        title={t.title}
        actions={
          <a href="/insights/rf" class="text-accent hover:underline">
            {t.open}
          </a>
        }
      />
      <Show
        when={bands().length > 0}
        fallback={
          <p role="status" class="px-4 pb-4 text-fg-subtle text-sm">
            {view() ? t.empty : `${strings.rf.loading}…`}
          </p>
        }
      >
        <ul class="grid gap-3 px-4 pb-4 sm:grid-cols-2 lg:grid-cols-3">
          <For each={bands()}>
            {(band) => {
              const s = () => bandSummary(band);
              return (
                <li class="flex flex-col gap-2 rounded-control border border-edge p-3">
                  <div class="flex items-center justify-between gap-2">
                    <a
                      href={`/insights/rf${band.band === "5g" ? "" : `?band=${band.band}`}`}
                      class="font-medium hover:text-accent hover:underline"
                    >
                      {strings.device.radios.band[band.band]}
                    </a>
                    <span class="font-mono text-fg-muted text-xs">
                      {s()
                        .channels.map((c) => strings.rf.radios.channel(c))
                        .join(", ")}
                    </span>
                  </div>
                  <Show when={s().busiest}>
                    {(r) => (
                      <p class="flex items-center gap-1.5 text-sm">
                        <StatusDot tone={busyTone(r().busy_percent)} />
                        <span class="font-mono tabular-nums">
                          {formatPercent(r().busy_percent)}
                        </span>
                        <span class="truncate text-fg-muted text-xs">
                          {`${t.busiest} · ${props.nodeName(r().node)}`}
                        </span>
                      </p>
                    )}
                  </Show>
                  <p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-fg-muted text-xs">
                    <span>{t.radios(s().radios)}</span>
                    <span>{t.neighbors(s().neighbors)}</span>
                    <Show when={s().crowded > 0}>
                      <Badge tone="crit">{t.crowded(s().crowded)}</Badge>
                    </Show>
                    <Show when={s().lookalikes > 0}>
                      <Badge tone="warn">{t.lookalikes(s().lookalikes)}</Badge>
                    </Show>
                  </p>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
    </Card>
  );
}

import { createSignal, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import type { ChannelReport } from "../../api/gen/ChannelReport";
import { formatDbm, formatPercent, NONE } from "../../lib/format";
import { busyTone, relative } from "../../lib/rf";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { Segmented } from "../../ui/Segmented";
import { type Column, Table } from "../../ui/Table";

const t = strings.rf.channels;

const BAR: Record<Tone, string> = {
  ok: "fill-ok",
  warn: "fill-warn",
  crit: "fill-crit",
  info: "fill-info",
  neutral: "fill-fg-subtle",
};

/** A busy share as a small bar and its number. */
function Busy(props: { percent: number | null }) {
  return (
    <span class="inline-flex items-center justify-end gap-2">
      <Show when={props.percent !== null}>
        <svg viewBox="0 0 60 6" class="h-1.5 w-15" aria-hidden="true">
          <rect width="60" height="6" rx="3" class="fill-raised" />
          <rect
            width={Math.max(2, Math.min(60, ((props.percent ?? 0) / 100) * 60))}
            height="6"
            rx="3"
            class={BAR[busyTone(props.percent)]}
          />
        </svg>
      </Show>
      <span class="w-10">{formatPercent(props.percent)}</span>
    </span>
  );
}

/** A channel in use: by the site's radios, by neighbors, or by radar lately. */
export function inUse(c: ChannelReport): boolean {
  return c.ours.length > 0 || c.networks > 0 || c.radar_hits > 0;
}

/**
 * Each 20 MHz channel: how busy, the neighbors on it, DFS and radar, and which radios use it.
 * Those in use by default; all of the band's on demand.
 */
export function Channels(props: {
  band: BandReport;
  nodeName: (id: number) => string;
  now: number;
}) {
  const [shown, setShown] = createSignal<"used" | "all">("used");
  const rows = () => (shown() === "all" ? props.band.channels : props.band.channels.filter(inUse));
  const columns: Column<ChannelReport>[] = [
    {
      id: "channel",
      header: t.channel,
      class: "w-40",
      cell: (c) => (
        <span class="flex items-center gap-2">
          <span class={["font-mono tabular-nums", c.ours.length > 0 ? "font-semibold" : ""]}>
            {c.channel}
          </span>
          <Show when={c.dfs}>
            <Badge tone={c.dfs_state === "unavailable" ? "crit" : "warn"}>
              {c.dfs_state ? t.dfsState[c.dfs_state] : t.dfs}
            </Badge>
          </Show>
          <Show when={!c.allowed}>
            <span class="text-fg-subtle text-xs">{t.notAllowed}</span>
          </Show>
        </span>
      ),
    },
    {
      id: "busy",
      header: t.busy,
      align: "end",
      class: "w-36",
      cell: (c) => <Busy percent={c.busy_percent} />,
    },
    {
      id: "networks",
      header: t.networks,
      align: "end",
      class: "w-20",
      cell: (c) => (c.networks > 0 ? c.networks : NONE),
    },
    {
      id: "strongest",
      header: t.strongest,
      align: "end",
      class: "w-24",
      cell: (c) => formatDbm(c.strongest_dbm),
    },
    {
      id: "ours",
      header: t.ours,
      cell: (c) =>
        c.ours.length === 0 ? (
          <span class="text-fg-subtle">{NONE}</span>
        ) : (
          <span class="whitespace-normal text-xs">
            {/* The radios whose primary channel it is first; those it only widens, muted. */}
            {[...c.ours]
              .sort((a, b) => Number(b.primary) - Number(a.primary))
              .map((u, i) => (
                <>
                  {i > 0 ? ", " : ""}
                  <span
                    class={u.primary ? "font-medium" : "text-fg-muted"}
                    title={u.primary ? t.primary : undefined}
                  >
                    {props.nodeName(u.node)}
                  </span>
                </>
              ))}
          </span>
        ),
    },
    {
      id: "radar",
      header: strings.rf.spectrum.radar,
      class: "w-52",
      cell: (c) =>
        c.radar_hits > 0 && c.last_radar !== null ? (
          <span class="text-xs">{t.radar(c.radar_hits, relative(c.last_radar, props.now))}</span>
        ) : (
          <span class="text-fg-subtle">{NONE}</span>
        ),
    },
  ];
  return (
    <Card class="min-w-0" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Segmented
            label={t.filterLabel}
            value={shown()}
            onChange={setShown}
            options={[
              { value: "used", label: t.filter.used },
              { value: "all", label: t.filter.all },
            ]}
          />
        }
      />
      <div class="px-2 pb-2">
        <Table
          label={t.title}
          columns={
            props.band.channels.some((c) => c.radar_hits > 0) ? columns : columns.slice(0, 5)
          }
          rows={rows()}
          rowKey={(c) => String(c.frequency_mhz)}
          empty={<p class="py-8 text-center text-fg-subtle text-sm">{t.unused}</p>}
        />
      </div>
    </Card>
  );
}

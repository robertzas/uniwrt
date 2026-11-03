import { createMemo, For, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import type { Neighbor } from "../../api/gen/Neighbor";
import { formatDbm } from "../../lib/format";
import { strings } from "../../lib/strings";
import { GRID_DBM, HEIGHT, hill, labelled, PAD, scale, span, WIDTH } from "./spectrum";

const t = strings.rf.spectrum;
const LABELS = 10;
/** The least room between two channel numbers on the axis, in chart units. */
const TICK_GAP = 26;

const SHAPE: Record<Neighbor["kind"], string> = {
  own: "fill-accent/25 stroke-accent",
  lookalike: "fill-warn/20 stroke-warn",
  foreign: "fill-fg-muted/10 stroke-fg-muted/70",
};

/** A network's name on the chart. */
export function networkName(network: Neighbor, nodeName: (id: number) => string): string {
  if (network.kind === "own" && network.node !== null) {
    const node = nodeName(network.node);
    return network.bss.mesh_id === null ? node : `${node} (mesh)`;
  }
  if (network.bss.ssid !== null) return network.bss.ssid;
  return network.bss.mesh_id === null
    ? strings.rf.networks.hidden
    : strings.rf.networks.mesh(network.bss.mesh_id);
}

/**
 * The band's networks as hills over the channels they occupy, as high as they are heard; the
 * site's own in the accent color, lookalikes in warning, neighbors muted. Under the axis, DFS
 * channels are marked and radar hits flagged. Hovering or focusing a hill selects its network.
 */
export function Spectrum(props: {
  band: BandReport;
  nodeName: (id: number) => string;
  selected: string | undefined;
  onSelect: (bssid: string | undefined) => void;
}) {
  const s = createMemo(() => scale(props.band));
  // The strongest last, drawn on top; the selected one above all.
  const drawn = createMemo(() =>
    [...props.band.networks].sort((a, b) => a.bss.signal_dbm - b.bss.signal_dbm),
  );
  const labels = createMemo(() => {
    const wanted = labelled(props.band.networks, LABELS);
    const placed: { x: number; y: number; text: string; bssid: string }[] = [];
    for (const n of [...drawn()].reverse()) {
      if (!wanted.has(n.bss.bssid)) continue;
      const x = s().x(n.bss.center_frequency_mhz);
      const y = s().y(n.bss.signal_dbm) - 5;
      const text = networkName(n, props.nodeName);
      if (placed.some((p) => p.text === text && Math.abs(p.x - x) < 80)) continue;
      if (placed.some((p) => Math.abs(p.x - x) < 70 && Math.abs(p.y - y) < 12)) continue;
      placed.push({ x, y, text, bssid: n.bss.bssid });
    }
    return placed;
  });
  const selectedNetwork = () => props.band.networks.find((n) => n.bss.bssid === props.selected);
  // The channels numbered on the axis: the site's own first, then any that keep their
  // neighbors' numbers clear (radios can list channels only 5 MHz apart).
  const ticks = createMemo(() => {
    const placed: number[] = [];
    const fits = (x: number) => placed.every((p) => Math.abs(p - x) >= TICK_GAP);
    const channels = [...props.band.channels].sort(
      (a, b) => b.ours.length - a.ours.length || a.frequency_mhz - b.frequency_mhz,
    );
    const shown = new Set<number>();
    for (const c of channels) {
      const x = s().x(c.frequency_mhz);
      if (fits(x)) {
        placed.push(x);
        shown.add(c.frequency_mhz);
      }
    }
    return shown;
  });

  return (
    <div class="flex flex-col gap-2">
      <div class="-mx-1 overflow-x-auto px-1">
        <svg
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          class="h-auto w-full min-w-[40rem] select-none"
          role="img"
          aria-label={t.label(strings.device.radios.band[props.band.band])}
          onMouseLeave={() => props.onSelect(undefined)}
        >
          <For each={GRID_DBM}>
            {(dbm) => (
              <g>
                <line
                  x1={PAD.left}
                  x2={WIDTH - PAD.right}
                  y1={s().y(dbm)}
                  y2={s().y(dbm)}
                  class="stroke-edge"
                  stroke-dasharray="2 4"
                />
                <text
                  x={PAD.left - 6}
                  y={s().y(dbm) + 3}
                  text-anchor="end"
                  class="fill-fg-subtle font-mono text-[10px]"
                >
                  {dbm}
                </text>
              </g>
            )}
          </For>
          <For each={drawn()}>
            {(n) => (
              // biome-ignore lint/a11y/useSemanticElements: an SVG shape cannot be a button
              <path
                d={hill(n, s())}
                class={[
                  SHAPE[n.kind],
                  "cursor-pointer outline-none transition-[stroke-width] focus-visible:stroke-[3]",
                  props.selected === n.bss.bssid ? "stroke-[2.5]" : "stroke-1",
                ]}
                role="button"
                tabindex="0"
                aria-label={`${networkName(n, props.nodeName)} · ${n.bss.channel} · ${formatDbm(n.bss.signal_dbm)}`}
                onMouseEnter={() => props.onSelect(n.bss.bssid)}
                onFocus={() => props.onSelect(n.bss.bssid)}
              />
            )}
          </For>
          <For each={labels()}>
            {(label) => (
              <text
                x={label.x}
                y={label.y}
                text-anchor="middle"
                class={[
                  "pointer-events-none text-[11px]",
                  props.selected === label.bssid ? "fill-fg font-semibold" : "fill-fg-muted",
                ]}
              >
                {label.text}
              </text>
            )}
          </For>
          <line
            x1={PAD.left}
            x2={WIDTH - PAD.right}
            y1={s().base}
            y2={s().base}
            class="stroke-edge-strong"
          />
          <For each={s().breaks}>
            {(x) => (
              <line
                x1={x}
                x2={x}
                y1={PAD.top}
                y2={s().base + 30}
                class="stroke-edge"
                stroke-dasharray="1 3"
              />
            )}
          </For>
          <For each={props.band.channels}>
            {(c) => {
              const [low, high] = span(c.frequency_mhz, 20);
              const x0 = () => s().x(low) + 1;
              const x1 = () => s().x(high) - 1;
              return (
                <g>
                  <Show when={ticks().has(c.frequency_mhz)}>
                    <text
                      x={s().x(c.frequency_mhz)}
                      y={s().base + 14}
                      text-anchor="middle"
                      class={[
                        "font-mono text-[10px]",
                        c.ours.length > 0 ? "fill-accent font-semibold" : "fill-fg-subtle",
                      ]}
                    >
                      {c.channel}
                    </text>
                  </Show>
                  <Show when={c.dfs}>
                    <rect
                      x={x0()}
                      y={s().base + 20}
                      width={Math.max(x1() - x0(), 1)}
                      height={4}
                      rx={1}
                      class={c.dfs_state === "unavailable" ? "fill-crit/70" : "fill-warn/40"}
                    >
                      <title>{`${t.dfs} ${c.channel}`}</title>
                    </rect>
                  </Show>
                  <Show when={c.radar_hits > 0}>
                    <circle cx={s().x(c.frequency_mhz)} cy={s().base + 32} r={3} class="fill-crit">
                      <title>{`${t.radar} · ${c.channel}`}</title>
                    </circle>
                  </Show>
                </g>
              );
            }}
          </For>
        </svg>
      </div>
      <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 text-xs">
        <ul class="flex flex-wrap items-center gap-x-4 gap-y-1 text-fg-muted">
          <For each={["own", "lookalike", "foreign"] as const}>
            {(kind) => (
              <li class="flex items-center gap-1.5">
                <svg viewBox="0 0 12 12" class="size-3" aria-hidden="true">
                  <rect x="1" y="1" width="10" height="10" rx="2" class={SHAPE[kind]} />
                </svg>
                {t.legend[kind]}
              </li>
            )}
          </For>
          <Show when={props.band.channels.some((c) => c.dfs)}>
            <li class="flex items-center gap-1.5">
              <span class="h-1 w-3 rounded-sm bg-warn/40" aria-hidden="true" />
              {t.dfs}
            </li>
          </Show>
          <Show when={props.band.channels.some((c) => c.radar_hits > 0)}>
            <li class="flex items-center gap-1.5">
              <span class="size-1.5 rounded-full bg-crit" aria-hidden="true" />
              {t.radar}
            </li>
          </Show>
        </ul>
        <p class="min-h-4 text-fg-muted" aria-live="polite">
          <Show when={selectedNetwork()}>
            {(n) => (
              <span>
                <span class="font-medium text-fg">{networkName(n(), props.nodeName)}</span>
                {` · ${strings.rf.radios.channel(n().bss.channel)} · ${n().bss.width} MHz · ${formatDbm(n().bss.signal_dbm)}`}
                <Show when={n().vendor}>{(v) => ` · ${v()}`}</Show>
              </span>
            )}
          </Show>
        </p>
      </div>
    </div>
  );
}

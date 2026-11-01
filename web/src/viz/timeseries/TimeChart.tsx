import "uplot/dist/uPlot.min.css";
import type { JSX } from "@solidjs/web";
import { createEffect, createMemo, createSignal, For, onSettled, Show } from "solid-js";
import type uPlot from "uplot";
import { formatDateTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { resolveColors, token, watchTheme } from "../theme";
import {
  type Aligned,
  align,
  hasData,
  type Line,
  latest,
  type ScaleHints,
  valueAt,
  valueRange,
} from "./data";

export interface TimeChartProps extends ScaleHints {
  title: string;
  /** Beside the title: a picker for what the chart shows. */
  controls?: JSX.Element;
  lines: readonly Line[];
  format: (value: number) => string;
  /** The time axis (ms); from the data where absent. */
  from?: number | undefined;
  to?: number | undefined;
  /** Keeps the value axis to whole steps (counts). */
  whole?: boolean | undefined;
  height?: number | undefined;
  /** Charts with the same key share the cursor. */
  sync?: string | undefined;
  /** Lets the user drag across the chart to pick a window (ms). */
  onZoom?: ((from: number, to: number) => void) | undefined;
  loading?: boolean | undefined;
  /** What the chart says when there is no data (`strings.chart.empty` by default). */
  empty?: string | undefined;
}

/** What the uPlot callbacks read: the latest props, kept in a plain variable (no tracked reads). */
interface Current extends ScaleHints {
  aligned: Aligned;
  lines: readonly Line[];
  format: (value: number) => string;
  from: number | undefined;
  to: number | undefined;
  whole: boolean;
  sync: string | undefined;
  onZoom: ((from: number, to: number) => void) | undefined;
  /** What a change of needs a new plot rather than new data. */
  shape: string;
}

const AXIS = 64;
const WHOLE_STEPS = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000, 10_000];

/**
 * A time-series chart on uPlot (loaded on first use): lines over a light grid, min/max bands where
 * points stand for several values, an HTML legend with every line's value at the pointer (or the
 * latest), cursors shared between charts with one `sync` key, and drag-to-zoom with `onZoom`.
 * The canvas follows the skin and theme. Without a canvas (tests), only the legend renders.
 */
export function TimeChart(props: TimeChartProps) {
  const [hover, setHover] = createSignal<number | null>(null);
  const aligned = createMemo(() => align(props.lines));
  const empty = () => !hasData(aligned());
  let host: HTMLDivElement | undefined;
  let plot: uPlot | undefined;
  let current: Current | undefined;

  const build = (UPlot: typeof uPlot) => {
    plot?.destroy();
    plot = undefined;
    if (!host || !current) return;
    const latest = current;
    // The callbacks read the props of the moment, not those the plot was built with.
    plot = new UPlot(
      options(host, () => current ?? latest, setHover),
      data(current.aligned),
      host,
    );
  };

  let library: typeof uPlot | undefined;
  createEffect(
    () => ({
      aligned: aligned(),
      lines: props.lines,
      format: props.format,
      min: props.min,
      max: props.max,
      minSpan: props.minSpan,
      from: props.from,
      to: props.to,
      whole: props.whole ?? false,
      sync: props.sync,
      onZoom: props.onZoom,
      shape: JSON.stringify([
        props.lines.map((l) => [l.label, l.color, Boolean(l.min && l.max), l.area ?? false]),
        props.sync ?? null,
        Boolean(props.onZoom),
        props.whole ?? false,
        props.height ?? 160,
      ]),
    }),
    (next) => {
      const reshape = current?.shape !== next.shape;
      current = next;
      if (!library) return;
      if (reshape || !plot) build(library);
      else plot.setData(data(next.aligned));
    },
  );

  onSettled(() => {
    const canvas = document.createElement("canvas").getContext("2d");
    if (!canvas || !host) return;
    const target = host;
    let disposed = false;
    void import("uplot").then(({ default: UPlot }) => {
      if (disposed) return;
      library = UPlot;
      build(UPlot);
    });
    const resize = new ResizeObserver(() => {
      if (plot) plot.setSize({ width: target.clientWidth, height: target.clientHeight });
    });
    resize.observe(target);
    const unwatch = watchTheme(() => library && build(library));
    return () => {
      disposed = true;
      resize.disconnect();
      unwatch();
      plot?.destroy();
      plot = undefined;
    };
  });

  const shown = (index: number) => {
    const column = aligned().values[index] ?? [];
    const at = hover();
    const value = at === null ? latest(column) : valueAt(column, at);
    return value === null ? "—" : props.format(value);
  };

  return (
    <figure class="flex min-w-0 flex-col gap-2">
      <figcaption class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <span class="flex min-w-0 items-center gap-2">
          <span class="font-medium text-sm">{props.title}</span>
          {props.controls}
        </span>
        <span class="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-xs">
          <Show when={hover() !== null}>
            <span class="text-fg-subtle tabular-nums">
              {formatDateTime(aligned().times[hover() ?? 0] ?? 0)}
            </span>
          </Show>
          <For each={props.lines}>
            {(line, index) => (
              <span class="inline-flex items-center gap-1.5">
                <span
                  aria-hidden="true"
                  class="size-2 rounded-full"
                  style={{ background: token(line.color) }}
                />
                <span class="text-fg-subtle">{line.label}</span>
                <span class="font-medium tabular-nums">{shown(index())}</span>
              </span>
            )}
          </For>
        </span>
      </figcaption>
      <div
        ref={(el) => {
          host = el;
        }}
        role="img"
        aria-label={props.title}
        class={[
          "relative min-w-0 touch-pan-y select-none [&_.u-select]:bg-accent/15",
          // Without data an axis says nothing: only the message shows.
          empty() && "[&_.uplot]:invisible",
        ]}
        style={{ height: `${props.height ?? 160}px` }}
      >
        <Show when={empty()}>
          <p
            role="status"
            class="pointer-events-none absolute inset-0 z-10 grid place-items-center text-fg-subtle text-sm"
          >
            {props.loading ? `${strings.chart.loading}…` : (props.empty ?? strings.chart.empty)}
          </p>
        </Show>
      </div>
    </figure>
  );
}

function data(aligned: Aligned): uPlot.AlignedData {
  const columns: (number | null | undefined)[][] = [];
  aligned.values.forEach((values, i) => {
    columns.push(values);
    const band = aligned.bands[i];
    if (band) columns.push(band.max, band.min);
  });
  return [aligned.times, ...columns];
}

function options(
  host: HTMLElement,
  get: () => Current,
  setHover: (index: number | null) => void,
): uPlot.Options {
  // What decides the plot's shape is fixed for its life (a new shape builds a new plot).
  const current = get();
  const colors: Record<string, string> = resolveColors(host, {
    grid: token("edge"),
    axis: token("fg-subtle"),
    surface: token("surface"),
    ...Object.fromEntries(
      current.lines.flatMap((line, i) => [
        [`line${i}`, token(line.color)],
        [`tint${i}`, token(line.color, line.area ? 0.12 : 0.18)],
      ]),
    ),
  });
  const font = `11px ${getComputedStyle(host).fontFamily || "sans-serif"}`;
  const series: uPlot.Series[] = [{}];
  const bands: uPlot.Band[] = [];
  const envelopes = new Set<number>();
  current.lines.forEach((line, i) => {
    series.push({
      label: line.label,
      stroke: colors[`line${i}`] ?? "currentColor",
      width: 1.75,
      ...(line.area && { fill: colors[`tint${i}`] }),
      // uPlot shows the points only where they have room: sparse data stays visible.
      points: { size: 5, fill: colors.surface },
    });
    if (line.min && line.max) {
      const upper = series.length;
      for (const label of ["max", "min"])
        series.push({
          label: `${line.label} ${label}`,
          stroke: "transparent",
          width: 0,
          points: { show: false },
        });
      envelopes.add(upper).add(upper + 1);
      bands.push({ series: [upper, upper + 1], fill: colors[`tint${i}`] });
    }
  });
  const axis = (values: uPlot.Axis["values"]): uPlot.Axis => ({
    stroke: colors.axis,
    font,
    grid: { stroke: colors.grid, width: 1 },
    ticks: { show: false },
    ...(values && { values }),
  });
  return {
    width: host.clientWidth,
    height: host.clientHeight,
    ms: 1,
    legend: { show: false },
    padding: [8, 8, 0, 0],
    scales: {
      x: {
        time: true,
        range: (_u, low, high) => [get().from ?? low, get().to ?? high],
      },
      y: {
        range: (_u, low, high) => valueRange(low, high, get()),
      },
    },
    axes: [
      { ...axis(undefined), space: 80 },
      {
        ...axis((_u, splits) => distinct(splits.map((v) => get().format(v)))),
        size: AXIS,
        ...(current.whole && { incrs: WHOLE_STEPS }),
      },
    ],
    series,
    bands,
    cursor: {
      ...(current.sync && { sync: { key: current.sync } }),
      drag: { x: Boolean(current.onZoom), y: false, setScale: false },
      points: {
        size: (_u, i) => (envelopes.has(i) ? 0 : 7),
        width: 2,
        fill: colors.surface,
      },
    },
    hooks: {
      setCursor: [(u) => setHover(u.cursor.idx ?? null)],
      setSelect: [
        (u) => {
          const { left, width } = u.select;
          const onZoom = get().onZoom;
          if (width > 4 && onZoom) {
            onZoom(u.posToVal(left, "x"), u.posToVal(left + width, "x"));
          }
          if (width > 0) u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
        },
      ],
    },
  };
}

/** Axis labels with repeats blanked: rounding can give neighbouring ticks the same text. */
function distinct(labels: string[]): string[] {
  return labels.map((label, i) => (i > 0 && labels[i - 1] === label ? "" : label));
}

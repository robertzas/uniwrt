import { createMemo, createSignal, For, Show } from "solid-js";
import { formatTime } from "../../lib/format";

/** One line: its name, its color (a text color class, drawn as `currentColor`) and values. */
export interface Series {
  label: string;
  class: string;
  /** One per time; `null` leaves a gap. */
  values: readonly (number | null)[];
}

export interface LineChartProps {
  title: string;
  /** Milliseconds since the epoch, ascending, one per value. */
  times: readonly number[];
  series: readonly Series[];
  format: (value: number) => string;
  /** The scale's bounds; from the data (with some room) where absent. */
  min?: number;
  max?: number;
  /** The least the scale spans, so that small wobbles don't fill the chart (20 dB of signal). */
  minSpan?: number;
}

const W = 600;
const H = 160;
const TICKS = 4;

/**
 * A small time-series chart: lines over a light grid, the latest values in the legend, and a
 * crosshair with every series' value at the pointer. Lines are SVG scaled to the width; labels
 * are HTML, so text never stretches.
 */
export function LineChart(props: LineChartProps) {
  const [hover, setHover] = createSignal<number | null>(null);
  const domain = createMemo(() => {
    const values = props.series.flatMap((s) => s.values.filter((v): v is number => v != null));
    let min = props.min ?? Math.min(...values);
    let max = props.max ?? Math.max(...values);
    if (!Number.isFinite(min) || !Number.isFinite(max)) return { min: 0, max: 1 };
    const least = Math.max(props.minSpan ?? 0, 2);
    if (max - min < least) {
      const middle = (min + max) / 2;
      min = middle - least / 2;
      max = middle + least / 2;
    }
    const room = (max - min) * 0.1;
    return {
      min: props.min ?? min - room,
      max: props.max ?? max + room,
    };
  });
  const start = () => props.times[0] ?? 0;
  const span = () => Math.max(1, (props.times[props.times.length - 1] ?? 0) - start());
  const x = (index: number) => (((props.times[index] ?? 0) - start()) / span()) * W;
  const y = (value: number) => {
    const { min, max } = domain();
    return H - ((Math.min(Math.max(value, min), max) - min) / (max - min)) * H;
  };
  const path = (values: readonly (number | null)[]) => {
    let d = "";
    let pen = false;
    values.forEach((value, index) => {
      if (value == null) {
        pen = false;
        return;
      }
      d += `${pen ? "L" : "M"}${x(index).toFixed(1)} ${y(value).toFixed(1)}`;
      pen = true;
    });
    return d;
  };
  const ticks = () => {
    const { min, max } = domain();
    return Array.from({ length: TICKS + 1 }, (_, i) => min + ((max - min) * i) / TICKS);
  };
  const latest = (values: readonly (number | null)[]) =>
    [...values].reverse().find((value): value is number => value != null);
  const onMove = (event: PointerEvent) => {
    const target = event.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    if (rect.width === 0 || props.times.length === 0) return;
    const time = start() + ((event.clientX - rect.left) / rect.width) * span();
    let best = 0;
    props.times.forEach((t, i) => {
      if (Math.abs(t - time) < Math.abs((props.times[best] ?? 0) - time)) best = i;
    });
    setHover(best);
  };

  return (
    <figure class="flex flex-col gap-2">
      <figcaption class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <span class="font-medium text-sm">{props.title}</span>
        <span class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
          <For each={props.series}>
            {(series) => (
              <span class="inline-flex items-center gap-1.5">
                <span aria-hidden="true" class={["size-2 rounded-full bg-current", series.class]} />
                <span class="text-fg-subtle">{series.label}</span>
                <span class="font-medium tabular-nums">
                  {(() => {
                    const index = hover();
                    const value = index === null ? latest(series.values) : series.values[index];
                    return value == null ? "—" : props.format(value);
                  })()}
                </span>
              </span>
            )}
          </For>
        </span>
      </figcaption>
      <div class="flex gap-2">
        <div
          class="relative w-16 shrink-0 text-right text-fg-subtle text-xs tabular-nums"
          aria-hidden="true"
        >
          <For each={ticks()}>
            {(tick) => (
              <span
                class="-translate-y-1/2 absolute right-0 whitespace-nowrap"
                style={{ top: `${(y(tick) / H) * 100}%` }}
              >
                {props.format(tick)}
              </span>
            )}
          </For>
        </div>
        <div
          class="relative h-40 min-w-0 flex-1 touch-none"
          onPointerMove={onMove}
          onPointerLeave={() => setHover(null)}
        >
          <svg
            viewBox={`0 0 ${W} ${H}`}
            preserveAspectRatio="none"
            role="img"
            aria-label={props.title}
            class="absolute inset-0 size-full overflow-visible"
          >
            <For each={ticks()}>
              {(tick) => (
                <line
                  x1="0"
                  x2={W}
                  y1={y(tick)}
                  y2={y(tick)}
                  class="stroke-edge"
                  vector-effect="non-scaling-stroke"
                />
              )}
            </For>
            <For each={props.series}>
              {(series) => (
                <path
                  d={path(series.values)}
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.75"
                  stroke-linejoin="round"
                  vector-effect="non-scaling-stroke"
                  class={series.class}
                />
              )}
            </For>
          </svg>
          <Show when={hover() !== null}>
            <span
              aria-hidden="true"
              class="pointer-events-none absolute inset-y-0 w-px bg-fg-subtle"
              style={{ left: `${(x(hover() ?? 0) / W) * 100}%` }}
            />
            <span
              class="pointer-events-none absolute top-0 rounded-control border border-edge bg-surface px-1.5 py-0.5 text-fg-muted text-xs shadow-overlay"
              style={{ left: `${Math.min((x(hover() ?? 0) / W) * 100, 80)}%` }}
            >
              {formatTime(props.times[hover() ?? 0] ?? 0)}
            </span>
          </Show>
        </div>
      </div>
      <div
        class="ml-18 flex justify-between text-fg-subtle text-xs tabular-nums"
        aria-hidden="true"
      >
        <span>{props.times.length > 0 ? formatTime(start()) : ""}</span>
        <span>{props.times.length > 0 ? formatTime(start() + span()) : ""}</span>
      </div>
    </figure>
  );
}

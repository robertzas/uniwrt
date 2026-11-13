import { createMemo, Show } from "solid-js";

/** A color stop: at `value` and above (up to the next stop) the color blends toward it. */
export interface Stop {
  value: number;
  rgb: readonly [number, number, number];
}

/** Values on a grid over the plan, row by row; `NaN` cells and those `mask` leaves out are clear. */
export interface HeatGrid {
  cols: number;
  rows: number;
  values: Float32Array;
  mask?: Uint8Array;
}

/** The color of `value` between `stops` (ascending by value), clamped at both ends. */
export function colorAt(stops: readonly Stop[], value: number): readonly [number, number, number] {
  const first = stops[0];
  const last = stops[stops.length - 1];
  if (!first || !last) return [0, 0, 0];
  if (value <= first.value) return first.rgb;
  if (value >= last.value) return last.rgb;
  for (let i = 1; i < stops.length; i++) {
    const hi = stops[i] as Stop;
    const lo = stops[i - 1] as Stop;
    if (value <= hi.value) {
      const k = (value - lo.value) / (hi.value - lo.value);
      const mix = (c: 0 | 1 | 2) => Math.round(lo.rgb[c] + k * (hi.rgb[c] - lo.rgb[c]));
      return [mix(0), mix(1), mix(2)];
    }
  }
  return last.rgb;
}

/** The grid as a PNG data URL, a pixel per cell; undefined where the browser can't paint. */
export function paint(grid: HeatGrid, stops: readonly Stop[], alpha: number): string | undefined {
  const canvas = document.createElement("canvas");
  canvas.width = grid.cols;
  canvas.height = grid.rows;
  const context = canvas.getContext("2d");
  if (!context) return undefined;
  const image = context.createImageData(grid.cols, grid.rows);
  const a = Math.round(alpha * 255);
  for (let i = 0; i < grid.values.length; i++) {
    const value = grid.values[i] ?? Number.NaN;
    if (Number.isNaN(value) || (grid.mask && !grid.mask[i])) continue;
    const [r, g, b] = colorAt(stops, value);
    image.data.set([r, g, b, a], i * 4);
  }
  context.putImageData(image, 0, 0);
  return canvas.toDataURL();
}

/**
 * A heatmap laid over a plan in plan space (inside `PlanCanvas`'s `underlay`), each cell blended
 * smoothly into its neighbors as the browser scales it up.
 */
export function Heatmap(props: {
  grid: HeatGrid;
  stops: readonly Stop[];
  width: number;
  height: number;
  alpha?: number;
}) {
  const url = createMemo(() => paint(props.grid, props.stops, props.alpha ?? 0.55));
  return (
    <Show when={url()}>
      {(href) => (
        <image
          href={href()}
          width={props.width}
          height={props.height}
          preserveAspectRatio="none"
          class="pointer-events-none"
          data-heatmap=""
        />
      )}
    </Show>
  );
}

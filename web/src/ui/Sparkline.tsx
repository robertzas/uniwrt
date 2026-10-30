import { createMemo, Show } from "solid-js";

const HEIGHT = 24;

export interface SparklineProps {
  values: readonly number[];
  /** What the line shows, for screen readers, e.g. "Throughput, last 30 minutes". */
  label: string;
  /** The top of the scale; the largest value plus 10 % when absent. */
  max?: number;
  /** Sets the color through `currentColor`, e.g. "text-accent". */
  class?: string;
}

/** A small trend line with a faint area fill, scaled to its container's width. */
export function Sparkline(props: SparklineProps) {
  const path = createMemo(() => {
    const values = props.values;
    if (values.length < 2) return undefined;
    const max = props.max ?? Math.max(...values, 0) * 1.1;
    const scale = max > 0 ? max : 1;
    const step = 100 / (values.length - 1);
    return values
      .map((v, i) => {
        const y = HEIGHT - (Math.min(Math.max(v, 0), scale) / scale) * (HEIGHT - 2);
        return `${i === 0 ? "M" : "L"}${(i * step).toFixed(2)} ${y.toFixed(2)}`;
      })
      .join("");
  });
  return (
    <svg
      viewBox={`0 0 100 ${HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={props.label}
      class={["block h-6 w-full overflow-visible", props.class]}
    >
      <Show when={path()}>
        {(d) => (
          <>
            <path d={`${d()}L100 ${HEIGHT}L0 ${HEIGHT}Z`} fill="currentColor" fill-opacity="0.14" />
            <path
              d={d()}
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linejoin="round"
              vector-effect="non-scaling-stroke"
            />
          </>
        )}
      </Show>
    </svg>
  );
}

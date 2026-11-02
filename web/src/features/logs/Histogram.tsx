import { createMemo, createSignal, For, onSettled, Show } from "solid-js";
import type { LogHistogram } from "../../api/gen/LogHistogram";
import { formatDateTime, formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { bucketAt, bucketWindow, peak } from "./histogram";
import { countsByLevel, LEVEL_FILL, LEVEL_SVG_FILL, LEVELS, type Level } from "./severity";

const t = strings.logs.histogram;
/** The plot's height in px: `h-16` (a class: a constant inline style would break the CSP). */
const HEIGHT = 64;
/** Bars are drawn from the bottom in this order: errors stand on the axis. */
const STACK: readonly Level[] = ["error", "warning", "info", "debug"];
const DAY = 86_400_000;

/** How many bars fit `width` pixels: about one per 8 px, 20 to 120. */
export function barsFor(width: number): number {
  return Math.max(20, Math.min(120, Math.floor(width / 8)));
}

/**
 * The severity histogram: one stacked bar per span of the range. Hovering a bar shows its counts;
 * clicking one, or dragging across several, zooms the page into that window.
 */
export function Histogram(props: {
  data: LogHistogram | undefined;
  failed: boolean;
  onWidth: (width: number) => void;
  onZoom: (from: number, to: number) => void;
}) {
  let box: HTMLDivElement | undefined;
  const [hover, setHover] = createSignal<number>();
  const [drag, setDrag] = createSignal<{ start: number; end: number }>();
  const top = createMemo(() => (props.data ? peak(props.data) : 1));
  const totals = createMemo(() => {
    const sums: Record<Level, number> = { error: 0, warning: 0, info: 0, debug: 0 };
    for (const bucket of props.data?.buckets ?? []) {
      const levels = countsByLevel(bucket.counts);
      for (const level of LEVELS) sums[level] += levels[level];
    }
    return sums;
  });
  const total = () => LEVELS.reduce((sum, level) => sum + totals()[level], 0);
  const label = (ms: number) => {
    const data = props.data;
    return data && data.to - data.from > DAY ? formatDateTime(ms) : formatTime(ms);
  };

  onSettled(() => {
    if (!box) return;
    const element = box;
    // The observer reports the first size too (jsdom has none: the page's default stands).
    const observer = new ResizeObserver(() => props.onWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  });

  const bucketOf = (event: PointerEvent) => {
    const data = props.data;
    if (!data || !box || data.buckets.length === 0) return undefined;
    const rect = box.getBoundingClientRect();
    return bucketAt((event.clientX - rect.left) / Math.max(1, rect.width), data.buckets.length);
  };
  const onDown = (event: PointerEvent) => {
    const at = bucketOf(event);
    if (at === undefined || event.button !== 0) return;
    (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
    setDrag({ start: at, end: at });
  };
  const onMove = (event: PointerEvent) => {
    const at = bucketOf(event);
    setHover(at);
    const current = drag();
    if (current && at !== undefined) setDrag({ ...current, end: at });
  };
  const onUp = () => {
    const current = drag();
    const data = props.data;
    setDrag(undefined);
    if (!current || !data) return;
    const window = bucketWindow(data, current.start, current.end);
    props.onZoom(window.from, window.to);
  };
  const selected = (index: number) => {
    const current = drag();
    if (!current) return false;
    return (
      index >= Math.min(current.start, current.end) && index <= Math.max(current.start, current.end)
    );
  };
  const shown = () => {
    const index = hover();
    const bucket = index === undefined ? undefined : props.data?.buckets[index];
    return index === undefined || !bucket || !props.data
      ? undefined
      : { index, bucket, levels: countsByLevel(bucket.counts), step: props.data.step_ms };
  };

  return (
    <figure class="flex flex-col gap-2">
      <figcaption class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 text-xs">
        <span class="font-medium text-fg-muted">{t.label}</span>
        <span class="flex flex-wrap items-center gap-x-3 gap-y-1 text-fg-subtle tabular-nums">
          <span>{t.total(total())}</span>
          <For each={LEVELS}>
            {(level) => (
              <span class="inline-flex items-center gap-1.5">
                <span aria-hidden="true" class={["size-2 rounded-sm", LEVEL_FILL[level]]} />
                {strings.logs.level[level]} {totals()[level].toLocaleString()}
              </span>
            )}
          </For>
        </span>
      </figcaption>
      <div
        ref={box}
        role="img"
        aria-label={t.summary(total(), totals().error, totals().warning)}
        class="relative h-16 touch-none select-none"
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={() => setDrag(undefined)}
        onPointerLeave={() => {
          setHover(undefined);
        }}
      >
        <Show
          when={props.data && props.data.buckets.length > 0}
          fallback={
            <div class="flex h-full items-center justify-center rounded-control border border-edge border-dashed text-fg-subtle text-xs">
              {props.failed ? strings.logs.failed : t.loading}
            </div>
          }
        >
          <svg
            class="block size-full cursor-crosshair overflow-visible"
            viewBox={`0 0 ${props.data?.buckets.length ?? 1} ${HEIGHT}`}
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <line
              x1="0"
              x2={props.data?.buckets.length ?? 1}
              y1={HEIGHT - 0.5}
              y2={HEIGHT - 0.5}
              stroke="var(--color-edge)"
              vector-effect="non-scaling-stroke"
            />
            <For each={props.data?.buckets ?? []}>
              {(bucket, index) => {
                const levels = () => countsByLevel(bucket.counts);
                const segments = () => {
                  let y = HEIGHT;
                  return STACK.filter((level) => levels()[level] > 0).map((level) => {
                    const h = Math.max(1.5, (levels()[level] / top()) * (HEIGHT - 4));
                    y -= h;
                    return { level, y, h };
                  });
                };
                return (
                  <g opacity={drag() && !selected(index()) ? 0.35 : hover() === index() ? 0.8 : 1}>
                    <rect
                      x={index()}
                      y={0}
                      width={1}
                      height={HEIGHT}
                      fill={
                        selected(index()) || hover() === index() ? "var(--color-raised)" : "none"
                      }
                    />
                    <For each={segments()}>
                      {(segment) => (
                        <rect
                          x={index() + 0.12}
                          width={0.76}
                          y={segment.y}
                          height={segment.h}
                          fill={LEVEL_SVG_FILL[segment.level]}
                        />
                      )}
                    </For>
                  </g>
                );
              }}
            </For>
          </svg>
        </Show>
        <Show when={shown()}>
          {(s) => (
            <div
              class="pointer-events-none absolute bottom-full z-10 mb-2 w-max -translate-x-1/2 rounded-control border border-edge bg-surface px-2.5 py-1.5 text-xs shadow-overlay glass:backdrop-blur-panel"
              style={{
                left: `${((s().index + 0.5) / (props.data?.buckets.length ?? 1)) * 100}%`,
              }}
            >
              <p class="font-medium text-fg tabular-nums">
                {label(s().bucket.start)} – {formatTime(s().bucket.start + s().step)}
              </p>
              <ul class="mt-1 flex flex-col gap-0.5 text-fg-muted tabular-nums">
                <For each={LEVELS}>
                  {(level) => (
                    <li class="flex items-center gap-1.5">
                      <span aria-hidden="true" class={["size-2 rounded-sm", LEVEL_FILL[level]]} />
                      <span class="flex-1">{strings.logs.level[level]}</span>
                      <span class="pl-3 text-fg">{s().levels[level].toLocaleString()}</span>
                    </li>
                  )}
                </For>
              </ul>
            </div>
          )}
        </Show>
      </div>
      <Show when={props.data}>
        {(data) => (
          <div class="flex justify-between text-[0.6875rem] text-fg-subtle tabular-nums">
            <span>{label(data().from)}</span>
            <span class="hidden sm:inline">{t.hint}</span>
            <span>{label(data().to)}</span>
          </div>
        )}
      </Show>
    </figure>
  );
}

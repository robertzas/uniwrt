/** The severity histogram's data: read for the page's filter and range, again while live. */
import { createEffect, createMemo, createSignal } from "solid-js";
import type { LogHistogram } from "../../api/gen/LogHistogram";
import { api } from "../../lib/api";
import { LOG_PRESETS, type LogQuery, type LogRange, windowOf, withWindow } from "./query";

/** How often a live range's histogram is read again: about as often as one bar changes. */
export function histogramEvery(range: LogRange): number | null {
  if (!("preset" in range)) return null;
  return LOG_PRESETS[range.preset] <= 6 * 3_600_000 ? 15_000 : 60_000;
}

export interface HistogramFeed {
  data(): LogHistogram | undefined;
  failed(): boolean;
}

/**
 * Reads the histogram of `query` in `buckets()` bars, and again every so often while the range
 * is live and the page visible. Kept in a plain signal so a refresh never holds a navigation
 * (D-048); the last answer stays while a new filter is read.
 */
export function createHistogramFeed(query: () => LogQuery, buckets: () => number): HistogramFeed {
  const [answer, setAnswer] = createSignal<LogHistogram>();
  const [failedKey, setFailedKey] = createSignal<string>();
  const key = createMemo(() => JSON.stringify({ query: query(), buckets: buckets() }));
  createEffect(key, (current) => {
    const { query: q, buckets: n } = JSON.parse(current) as { query: LogQuery; buckets: number };
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const every = histogramEvery(q.range);
    const read = async () => {
      clearTimeout(timer);
      if (!document.hidden) {
        const window = windowOf(q.range, Date.now());
        // A live range ends now, which the histogram needs to name for its last bar.
        const filter = withWindow(q.filter, { from: window.from, to: window.to ?? Date.now() });
        try {
          setAnswer(await api.logs.histogram(filter, n, controller.signal));
          setFailedKey(undefined);
        } catch {
          if (!controller.signal.aborted) setFailedKey(current);
        }
      }
      if (every !== null && !controller.signal.aborted) timer = setTimeout(read, every);
    };
    const onVisible = () => {
      if (!document.hidden && every !== null) void read();
    };
    document.addEventListener("visibilitychange", onVisible);
    void read();
    return () => {
      controller.abort();
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
  return { data: answer, failed: () => failedKey() === key() };
}

/** The bucket under `x` (0–1 across the plot), clamped to the histogram. */
export function bucketAt(x: number, count: number): number {
  return Math.min(count - 1, Math.max(0, Math.floor(x * count)));
}

/** The window of buckets `a` to `b` (either order), as a zoom. */
export function bucketWindow(
  histogram: LogHistogram,
  a: number,
  b: number,
): { from: number; to: number } {
  const first = Math.min(a, b);
  const last = Math.max(a, b);
  const from = histogram.from + first * histogram.step_ms;
  const to = Math.min(histogram.to, histogram.from + (last + 1) * histogram.step_ms);
  return { from, to };
}

/** The highest bar's total, for the scale; at least 1. */
export function peak(histogram: LogHistogram): number {
  let most = 1;
  for (const bucket of histogram.buckets) {
    const total = bucket.counts.reduce((sum, n) => sum + n, 0);
    if (total > most) most = total;
  }
  return most;
}

import { createEffect, createMemo, createSignal, onSettled } from "solid-js";
import type { Metric } from "../../api/gen/Metric";
import type { MetricSeries } from "../../api/gen/MetricSeries";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import type { Resolution } from "../../api/gen/Resolution";
import type { SeriesInfo } from "../../api/gen/SeriesInfo";
import { api } from "../../lib/api";
import { refreshEvery, resolveRange, type TimeRange, type Window } from "./ranges";

/** One entity's metrics to read. */
export interface FeedQuery {
  entity: string;
  metrics: readonly Metric[];
}

export interface FeedInput {
  queries: readonly FeedQuery[];
  range: TimeRange;
  /** About how many points each series keeps: the chart's width in pixels. */
  width: number;
}

/** Stored series, read and kept current for a range. */
export interface MetricsFeed {
  series(entity: string, metric: Metric): MetricSeries | undefined;
  /** The window of the latest answer. */
  window(): Window | undefined;
  resolution(): Resolution | undefined;
  /** Reading a new set of queries or range (the last answer stays meanwhile). */
  loading(): boolean;
  /** The last read failed; the next refresh tries again. */
  failed(): boolean;
}

/**
 * Reads the stored series of `input` (one request per entity) and reads them again as often as
 * the range changes ([`refreshEvery`]), while the page is visible. The answers go into a plain
 * signal, not a query, so a refresh never holds a navigation (D-048).
 */
export function createMetricsFeed(input: () => FeedInput | undefined): MetricsFeed {
  const [answer, setAnswer] = createSignal<{ key: string; responses: MetricsResponse[] }>();
  const [failedKey, setFailedKey] = createSignal<string>();
  const key = createMemo(() => {
    const value = input();
    return value && value.queries.length > 0 ? JSON.stringify(value) : undefined;
  });
  createEffect(key, (current) => {
    if (current === undefined) return;
    const { queries, range, width } = JSON.parse(current) as FeedInput;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let busy = false;
    const read = async () => {
      if (busy || controller.signal.aborted) return;
      busy = true;
      clearTimeout(timer);
      if (!document.hidden) {
        const { from, to } = resolveRange(range, Date.now());
        try {
          const responses = await Promise.all(
            queries.map((q) => api.metrics.get({ ...q, from, to, width }, controller.signal)),
          );
          setAnswer({ key: current, responses });
          setFailedKey(undefined);
        } catch {
          if (!controller.signal.aborted) setFailedKey(current);
        }
      }
      busy = false;
      const every = refreshEvery(range);
      if (every !== null && !controller.signal.aborted) timer = setTimeout(read, every);
    };
    const onVisible = () => {
      if (!document.hidden && refreshEvery(range) !== null) void read();
    };
    document.addEventListener("visibilitychange", onVisible);
    void read();
    return () => {
      controller.abort();
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  });
  const byMetric = createMemo(() => {
    const map = new Map<string, MetricSeries>();
    for (const response of answer()?.responses ?? [])
      for (const series of response.series) map.set(`${response.entity} ${series.metric}`, series);
    return map;
  });
  return {
    series: (entity, metric) => byMetric().get(`${entity} ${metric}`),
    window: () => {
      const first = answer()?.responses[0];
      return first && { from: first.from, to: first.to };
    },
    resolution: () => answer()?.responses[0]?.resolution,
    loading: () => key() !== undefined && answer()?.key !== key(),
    failed: () => key() !== undefined && failedKey() === key(),
  };
}

const LISTING_MS = 5 * 60_000;
/** While series are still appearing (a node's rates follow its gauges by a window or two). */
const EMPTY_LISTING_MS = 15_000;

/**
 * The series kept, for every entity or for `under` and those below it: read on mount, then every
 * few minutes (series come and go slowly), or every 15 s while there are none yet or their number
 * still changes. Empty when the first read fails.
 */
export function createSeriesListing(under?: string): () => SeriesInfo[] | undefined {
  const [listing, setListing] = createSignal<SeriesInfo[]>();
  onSettled(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let loaded = false;
    let count = -1;
    const read = async () => {
      let next = EMPTY_LISTING_MS;
      try {
        const fresh = await api.metrics.series(under, controller.signal);
        setListing(fresh);
        loaded = true;
        if (fresh.length > 0 && fresh.length === count) next = LISTING_MS;
        count = fresh.length;
      } catch {
        // Without a listing the charts say there is no history yet; the next read tries again.
        if (!controller.signal.aborted && !loaded) setListing([]);
      }
      if (!controller.signal.aborted) timer = setTimeout(read, next);
    };
    void read();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  });
  return listing;
}

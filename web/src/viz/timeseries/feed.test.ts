import { createRoot, flush } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import { api } from "../../lib/api";
import { createMetricsFeed, EMPTY_READ_MS } from "./feed";

/** A site answer: download with these points' times. */
function answer(at: number[]): MetricsResponse {
  return {
    entity: "site",
    from: 0,
    to: 1,
    resolution: "minute",
    series: [
      {
        metric: "down_bps",
        unit: "bits_per_second",
        at,
        avg: at.map(() => 1),
        min: null,
        max: null,
      },
    ],
  } as MetricsResponse;
}

describe("createMetricsFeed", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("reads an empty answer again within a window, then at the range's pace", async () => {
    const get = vi
      .spyOn(api.metrics, "get")
      .mockResolvedValueOnce(answer([]))
      .mockResolvedValue(answer([1_000]));
    const dispose = createRoot((dispose) => {
      createMetricsFeed(() => ({
        queries: [{ entity: "site", metrics: ["down_bps"] }],
        range: { preset: "24h" },
        width: 600,
      }));
      return dispose;
    });
    flush();
    await vi.advanceTimersByTimeAsync(0);
    expect(get).toHaveBeenCalledTimes(1);

    // Nothing stored yet: read again after a window, not the 24 h range's minute.
    await vi.advanceTimersByTimeAsync(EMPTY_READ_MS);
    expect(get).toHaveBeenCalledTimes(2);

    // Points now: back to a minute.
    await vi.advanceTimersByTimeAsync(EMPTY_READ_MS);
    expect(get).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(60_000 - EMPTY_READ_MS);
    expect(get).toHaveBeenCalledTimes(3);
    dispose();
  });
});

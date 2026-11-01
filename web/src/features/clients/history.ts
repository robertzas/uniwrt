import type { ClientSample } from "../../api/gen/ClientSample";
import type { ColorToken, Line } from "../../viz/timeseries/data";
import type { Preset } from "../../viz/timeseries/ranges";

/** The ranges of a client's charts: the last hour from memory, then the stored series. */
export const CLIENT_RANGES: readonly Preset[] = ["1h", "24h", "7d", "30d"];

const HOUR = 3_600_000;

/** One field of the in-memory samples as a chart line. */
export function sampleLine(
  samples: readonly ClientSample[],
  label: string,
  color: ColorToken,
  key: keyof Omit<ClientSample, "at">,
  area = false,
): Line {
  return {
    label,
    color,
    at: samples.map((sample) => sample.at),
    values: samples.map((sample) => sample[key]),
    area,
  };
}

/** The hour up to the latest sample: the time axis of the live charts. */
export function liveWindow(samples: readonly ClientSample[]): { from?: number; to?: number } {
  const last = samples[samples.length - 1]?.at;
  return last === undefined ? {} : { from: last - HOUR, to: last };
}

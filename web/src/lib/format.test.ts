import { describe, expect, it } from "vitest";
import {
  formatBitrate,
  formatBytes,
  formatDuration,
  formatPercent,
  formatRate,
  NONE,
} from "./format";

describe("format", () => {
  it("keeps the two largest units of a duration", () => {
    expect(formatDuration(0)).toBe("0s");
    expect(formatDuration(40)).toBe("40s");
    expect(formatDuration(3720)).toBe("1h 2m");
    expect(formatDuration(3 * 86_400 + 4 * 3600 + 59)).toBe("3d 4h");
    expect(formatDuration(86_400 + 30)).toBe("1d");
    expect(formatDuration(null)).toBe(NONE);
  });

  it("scales bytes and rates in powers of 1,000", () => {
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1_400_000_000)).toBe("1.4 GB");
    expect(formatRate(866_700)).toBe("867 Mb/s");
    expect(formatRate(6_500)).toBe("6.5 Mb/s");
    expect(formatRate(2_402_000)).toBe("2.4 Gb/s");
    expect(formatBitrate(840)).toBe("840 b/s");
    expect(formatBitrate(12_400)).toBe("12 kb/s");
    expect(formatBitrate(3_420_000)).toBe("3.4 Mb/s");
    expect(formatBitrate(1_200_000_000)).toBe("1.2 Gb/s");
    expect(formatBitrate(null)).toBe(NONE);
    expect(formatPercent(12.6)).toBe("13 %");
    expect(formatPercent(null)).toBe(NONE);
  });
});

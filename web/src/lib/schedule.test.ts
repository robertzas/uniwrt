import { describe, expect, it } from "vitest";
import { nextTime, ordered, spansOf, timeDraft, timeOf, timeText } from "./schedule";

const iso = (date: Date | undefined) => date?.toISOString();

describe("restart times", () => {
  // 2026-11-19 is a Thursday.
  const thursdayNoon = new Date("2026-11-19T12:00:00Z");

  it("comes on the next chosen day, today when its time is still ahead", () => {
    expect(iso(nextTime({ days: ["sun"], at: 240 }, "UTC", thursdayNoon))).toBe(
      "2026-11-22T04:00:00.000Z",
    );
    expect(iso(nextTime({ days: ["thu"], at: 13 * 60 }, "UTC", thursdayNoon))).toBe(
      "2026-11-19T13:00:00.000Z",
    );
    expect(iso(nextTime({ days: ["thu"], at: 12 * 60 }, "UTC", thursdayNoon))).toBe(
      "2026-11-26T12:00:00.000Z",
    );
    expect(iso(nextTime({ days: ["fri", "mon"], at: 0 }, "UTC", thursdayNoon))).toBe(
      "2026-11-20T00:00:00.000Z",
    );
  });

  it("follows the site's zone across daylight saving time", () => {
    // Berlin leaves summer time on Sunday 2026-10-25 at 03:00 (UTC+2 to UTC+1).
    const friday = new Date("2026-10-23T12:00:00Z");
    expect(iso(nextTime({ days: ["sat"], at: 240 }, "Europe/Berlin", friday))).toBe(
      "2026-10-24T02:00:00.000Z",
    );
    expect(iso(nextTime({ days: ["mon"], at: 240 }, "Europe/Berlin", friday))).toBe(
      "2026-10-26T03:00:00.000Z",
    );
    // The local day, not UTC's: 23:30 in New York is already Friday in UTC.
    const lateThursday = new Date("2026-11-20T04:30:00Z");
    expect(
      iso(nextTime({ days: ["thu"], at: 23 * 60 + 45 }, "America/New_York", lateThursday)),
    ).toBe("2026-11-20T04:45:00.000Z");
    // 02:30 doesn't exist on 2027-03-28 in Berlin: it comes as 03:30 summer time.
    const saturday = new Date("2027-03-27T12:00:00Z");
    expect(iso(nextTime({ days: ["sun"], at: 150 }, "Europe/Berlin", saturday))).toBe(
      "2027-03-28T01:30:00.000Z",
    );
  });

  it("has none without days or in a zone the browser lacks", () => {
    expect(nextTime({ days: [], at: 240 }, "UTC", thursdayNoon)).toBeUndefined();
    expect(nextTime({ days: ["sun"], at: 240 }, "Mars/Olympus", thursdayNoon)).toBeUndefined();
  });

  it("reads and checks what is typed", () => {
    expect(timeDraft(null)).toEqual({ days: ["sun"], at: "04:00" });
    expect(timeDraft({ days: ["sat"], at: 75 })).toEqual({ days: ["sat"], at: "01:15" });
    expect(timeOf({ days: ["sun", "mon"], at: "03:30" })).toEqual({
      days: ["mon", "sun"],
      at: 210,
    });
    expect(timeOf({ days: [], at: "03:30" })).toBe("Pick one day at least.");
    expect(timeOf({ days: ["sun"], at: "3" })).toBe("Times are HH:MM.");
    expect(timeText({ days: ["sat", "sun"], at: 240 })).toBe("Sat, Sun 04:00");
    expect(ordered(["sun", "wed", "mon"])).toEqual(["mon", "wed", "sun"]);
    expect(spansOf([])).toBe("Add a time, or turn the schedule off.");
    expect(spansOf([{ days: ["sun", "sat"], from: "23:00", to: "07:00" }])).toEqual([
      { days: ["sat", "sun"], from: 1380, to: 420 },
    ]);
  });
});

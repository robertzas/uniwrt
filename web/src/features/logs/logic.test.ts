import { describe, expect, it } from "vitest";
import type { LogRecord } from "../../api/gen/LogRecord";
import { logParams } from "../../lib/api";
import { bucketAt, bucketWindow } from "./histogram";
import { admitter, highlights, matchesText, parseText, tokens } from "./match";
import {
  EMPTY_FILTER,
  filterParams,
  parseParams,
  sameFilter,
  savedFilter,
  topicOf,
  windowOf,
  withWindow,
} from "./query";
import { countsByLevel, facilityName, levelOf } from "./severity";
import { compareRecords, cursorOf, mergeRecords } from "./tail";

function record(id: number, extra: Partial<LogRecord> = {}): LogRecord {
  return {
    id,
    received: 1_000 * id,
    node: 1,
    host: null,
    via: "agent",
    pattern: null,
    seq: id,
    time: 1_000 * id,
    facility: 3,
    severity: "info",
    program: "netifd",
    pid: null,
    message: `entry ${id}`,
    ...extra,
  };
}

describe("the page's query", () => {
  it("reads the URL and writes it back", () => {
    const query = parseParams({
      nodes: "3,x,3,0",
      hosts: "switch",
      programs: "hostapd, dnsmasq",
      min: "warning",
      q: "deauth",
      annotated: "1",
      range: "24h",
    });
    expect(query).toEqual({
      filter: {
        nodes: [3],
        hosts: ["switch"],
        programs: ["hostapd", "dnsmasq"],
        min: "warning",
        text: "deauth",
        annotated: true,
      },
      range: { preset: "24h" },
    });
    expect(filterParams(query.filter)).toEqual({
      nodes: "3",
      hosts: "switch",
      programs: "hostapd,dnsmasq",
      min: "warning",
      q: "deauth",
      annotated: "1",
    });
    expect(parseParams({ min: "loud", range: "2y" })).toEqual({
      filter: EMPTY_FILTER,
      range: { preset: "1h" },
    });
    expect(parseParams({ from: "100", to: "200" }).range).toEqual({ from: 100, to: 200 });
    expect(parseParams({ from: "200", to: "100" }).range).toEqual({ preset: "1h" });
  });

  it("asks the hub for what it can narrow, and the API for the rest", () => {
    expect(topicOf(EMPTY_FILTER)).toBe("logs");
    expect(topicOf({ ...EMPTY_FILTER, nodes: [3], min: "warning" })).toBe(
      "logs?node=3&min=warning",
    );
    expect(topicOf({ ...EMPTY_FILTER, hosts: ["sw-1.lan"] })).toBe("logs?host=sw-1.lan");
    // Two sources, or a name the topic can't carry: every source, filtered on the page.
    expect(topicOf({ ...EMPTY_FILTER, nodes: [3], hosts: ["sw"] })).toBe("logs");
    expect(topicOf({ ...EMPTY_FILTER, hosts: ["a b"] })).toBe("logs");
    expect(topicOf({ ...EMPTY_FILTER, min: "debug" })).toBe("logs");

    const filter = withWindow(
      { ...EMPTY_FILTER, nodes: [1, 2], text: " " },
      windowOf({ preset: "15m" }, 1_000_000),
    );
    expect(logParams(filter).toString()).toBe("nodes=1%2C2&from=100000");
    expect(
      logParams({ ...savedFilter({ ...EMPTY_FILTER, annotated: true, min: "error" }) }).toString(),
    ).toBe("min=error&annotated=true");
  });

  it("knows a saved search by what it finds", () => {
    expect(sameFilter({ ...EMPTY_FILTER, nodes: [2, 1] }, { ...EMPTY_FILTER, nodes: [1, 2] })).toBe(
      true,
    );
    expect(sameFilter({ ...EMPTY_FILTER, text: "a " }, { ...EMPTY_FILTER, text: "a" })).toBe(true);
    expect(sameFilter(EMPTY_FILTER, { ...EMPTY_FILTER, annotated: true })).toBe(false);
  });
});

describe("the full-text match", () => {
  // The store's cases (crates/store/src/repo/logs_tests.rs), on the page.
  const entries = [
    record(1, {
      program: "dnsmasq-dhcp",
      message: "DHCPACK(br-lan) 10.0.0.5 02:00:00:00:04:00 phone",
    }),
    record(2, {
      program: "hostapd",
      message: "wlan0: STA 02:00:00:00:04:00 IEEE 802.11: associated",
    }),
    record(3, { program: "hostapd", message: "wlan1: STA 02:00:00:00:05:00 deauthenticated" }),
    record(4, { program: null, message: "switch port 3 flapping" }),
    record(5, { program: "netifd", message: "Interface 'wan' is now up" }),
  ];
  const find = (text: string) =>
    entries
      .filter((e) => matchesText(e, parseText(text)))
      .map((e) => e.id)
      .reverse();

  it("finds words, phrases, prefixes and programs, and leaves out exclusions", () => {
    expect(find("02:00:00:00:04:00")).toEqual([2, 1]);
    expect(find("PHONE")).toEqual([1]);
    expect(find("assoc*")).toEqual([2]);
    expect(find("hostapd")).toEqual([3, 2]);
    expect(find("02:00:00:00:04:00 -dhcpack")).toEqual([2]);
    expect(find("-hostapd -netifd")).toEqual([4, 1]);
    expect(find('"is now up"')).toEqual([5]);
    expect(find('"now is"')).toEqual([]);
    expect(find('-- " ')).toHaveLength(5);
  });

  it("folds case and diacritics as unicode61 does", () => {
    expect(tokens("Café-Übung wlan0")).toEqual(["cafe", "ubung", "wlan0"]);
    expect(parseText('-"two words" pre* x')).toEqual([
      { tokens: ["two", "words"], prefix: false, negated: true },
      { tokens: ["pre"], prefix: true, negated: false },
      { tokens: ["x"], prefix: false, negated: false },
    ]);
  });

  it("marks the words found", () => {
    expect(highlights("STA associated, assoc ok", parseText("assoc* -ok"))).toEqual([
      [4, 9],
      [16, 21],
    ]);
    expect(highlights("no match", [])).toEqual([]);
  });

  it("admits live entries as the store would", () => {
    const admit = admitter(
      { ...EMPTY_FILTER, nodes: [1], hosts: ["sw"], min: "warning", text: "port" },
      { from: 2_000, to: null },
    );
    expect(admit(record(3, { severity: "error", message: "port down" }))).toBe(true);
    expect(admit(record(3, { node: null, host: "sw", severity: "warning", message: "port" }))).toBe(
      true,
    );
    expect(admit(record(3, { severity: "notice", message: "port down" }))).toBe(false);
    expect(admit(record(3, { node: 2, severity: "error", message: "port down" }))).toBe(false);
    expect(admit(record(1, { severity: "error", message: "port down" }))).toBe(false);
    expect(admitter({ ...EMPTY_FILTER, annotated: true }, { from: 0, to: 5_000 })(record(6))).toBe(
      false,
    );
  });
});

describe("the tail's entries", () => {
  it("merges arrivals newest first, each once, and never past unread ones", () => {
    const shown = [record(5), record(4), record(3)];
    expect(mergeRecords(shown, [record(6), record(5)]).map((r) => r.id)).toEqual([6, 5, 4, 3]);
    // A late entry (an older clock) takes its place.
    expect(mergeRecords(shown, [record(7, { time: 3_500 })]).map((r) => r.id)).toEqual([
      5, 4, 7, 3,
    ]);
    // Older than the oldest shown while more are unread: left for the next page.
    expect(mergeRecords(shown, [record(1)], record(3)).map((r) => r.id)).toEqual([5, 4, 3]);
    expect(mergeRecords(shown, [])).toBe(shown);
    expect(cursorOf(record(3))).toBe("3000:3");
    expect([record(1), record(2, { time: 1_000 })].sort(compareRecords).map((r) => r.id)).toEqual([
      2, 1,
    ]);
  });
});

describe("severities and the histogram", () => {
  it("bands severities and sums buckets", () => {
    expect(["critical", "warning", "notice", "debug"].map((s) => levelOf(s as never))).toEqual([
      "error",
      "warning",
      "info",
      "debug",
    ]);
    expect(countsByLevel([1, 0, 0, 2, 3, 4, 5, 6])).toEqual({
      error: 3,
      warning: 3,
      info: 9,
      debug: 6,
    });
    expect([3, 10, 19, 99].map(facilityName)).toEqual(["daemon", "authpriv", "local3", "99"]);
  });

  it("turns a drag into a window", () => {
    const histogram = { from: 0, to: 55_000, step_ms: 10_000, buckets: [] };
    expect(bucketAt(0.99, 6)).toBe(5);
    expect(bucketAt(-1, 6)).toBe(0);
    expect(bucketWindow(histogram, 3, 1)).toEqual({ from: 10_000, to: 40_000 });
    expect(bucketWindow(histogram, 5, 5)).toEqual({ from: 50_000, to: 55_000 });
  });
});

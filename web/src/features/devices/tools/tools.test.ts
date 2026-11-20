import { describe, expect, it } from "vitest";
import type { DiagRun } from "../../../api/gen/DiagRun";
import {
  cablePorts,
  describeTool,
  draftErrors,
  emptyDraft,
  isAddress,
  isHttpUrl,
  isLookupName,
  toolOf,
} from "./draft";
import { iperf3Flows } from "./iperf3";
import { applyDiag, emptyRunView, MAX_LINES } from "./run";

function run(extra: Partial<DiagRun> = {}): DiagRun {
  return {
    id: 7,
    node: 1,
    tool: { tool: "ping", target: "1.1.1.1", family: null, count: 10 },
    state: "running",
    by: "admin",
    started_at: 1_000,
    ended_at: null,
    exit_code: null,
    error: null,
    lines: [],
    dropped: 0,
    ...extra,
  };
}

const line = (text: string) => ({ at: 1, err: false, text });

describe("the run form's checks", () => {
  it("takes what the model takes", () => {
    for (const ok of [
      "1.1.1.1",
      "::1",
      "2001:db8::1",
      "openwrt.org",
      "_sip._tcp.example.com",
      "a.",
    ]) {
      expect(isAddress(ok) || isLookupName(ok), ok).toBe(true);
    }
    for (const bad of ["-c", "a..b", "", "a b", "1.1.1.1; reboot", "x".repeat(64)]) {
      expect(isAddress(bad) || isLookupName(bad), bad).toBe(false);
    }
    expect(isAddress("999.1.1.1")).toBe(false);
    expect(isHttpUrl("https://openwrt.org/")).toBe(true);
    expect(isHttpUrl("http://[::1]:8080")).toBe(true);
    expect(isHttpUrl("https://")).toBe(false);
    expect(isHttpUrl("ftp://openwrt.org")).toBe(false);
    expect(isHttpUrl("https://a b")).toBe(false);
  });

  it("builds each tool from its fields, trimmed", () => {
    const draft = { ...emptyDraft(), target: " openwrt.org ", count: "4", family: "ipv6" as const };
    expect(toolOf(draft)).toEqual({
      tool: "ping",
      target: "openwrt.org",
      family: "ipv6",
      count: 4,
    });
    expect(toolOf({ ...draft, tool: "dns", name: "openwrt.org", record: "MX" })).toEqual({
      tool: "dns",
      name: "openwrt.org",
      record: "MX",
      server: null,
    });
    expect(
      toolOf({ ...draft, tool: "iperf3", peer: "3", direction: "both", streams: "4" }),
    ).toEqual({
      tool: "iperf3",
      peer: { node: 3 },
      direction: "both",
      seconds: 10,
      streams: 4,
    });
    expect(toolOf({ ...draft, tool: "iperf3" })).toMatchObject({ peer: "controller" });
    expect(toolOf({ ...draft, tool: "cable_test", port: "lan1" })).toEqual({
      tool: "cable_test",
      port: "lan1",
    });
  });

  it("names the fields a tool can't run with", () => {
    expect(draftErrors({ ...emptyDraft(), target: "-f", count: "101" })).toEqual({
      target: expect.any(String),
      count: expect.any(String),
    });
    expect(draftErrors({ ...emptyDraft(), tool: "http" })).toHaveProperty("url");
    expect(draftErrors({ ...emptyDraft(), tool: "iperf3", seconds: "61", streams: "0" })).toEqual({
      seconds: expect.any(String),
      streams: expect.any(String),
    });
    // Fields of other tools don't count.
    expect(draftErrors({ ...emptyDraft(), tool: "traceroute", target: "x", count: "" })).toEqual(
      {},
    );
    expect(toolOf({ ...emptyDraft(), target: "" })).toBeUndefined();
  });

  it("offers the board's ports and words runs", () => {
    expect(cablePorts({ lan: ["lan1", "lan2"], wan: "wan" })).toEqual(["lan1", "lan2", "wan"]);
    expect(cablePorts({ lan: ["eth0"], wan: "eth0" })).toEqual(["eth0"]);
    expect(cablePorts(undefined)).toEqual([]);
    const name = (id: number) => `node ${id}`;
    expect(
      describeTool(
        { tool: "iperf3", peer: { node: 2 }, direction: "receive", seconds: 10, streams: 1 },
        name,
      ),
    ).toBe("Throughput from node 2");
    expect(describeTool({ tool: "dns", name: "a.b", record: null, server: null }, name)).toBe(
      "DNS lookup of a.b",
    );
  });
});

describe("iperf3's output", () => {
  const send = [
    "Connecting to host 192.168.1.3, port 5201",
    "[  5] local 192.168.1.1 port 41000 connected to 192.168.1.3 port 5201",
    "[ ID] Interval           Transfer     Bitrate         Retr  Cwnd",
    "[  5]   0.00-1.00   sec   112 MBytes   940 Mbits/sec    0   2.25 MBytes       ",
    "[  5]   1.00-2.00   sec   111 MBytes   931 Mbits/sec    3   2.25 MBytes       ",
    "- - - - - - - - - - - - - - - - - - - - - - - - -",
    "[ ID] Interval           Transfer     Bitrate         Retr",
    "[  5]   0.00-2.00   sec   223 MBytes   935 Mbits/sec    3             sender",
    "[  5]   0.00-2.00   sec   222 MBytes   932 Mbits/sec                  receiver",
    "",
    "iperf Done.",
  ];

  it("follows a test's seconds, then both ends' totals", () => {
    const [flow, ...rest] = iperf3Flows(send, { direction: "send", streams: 1 });
    expect(rest).toEqual([]);
    expect(flow).toEqual({
      way: "up",
      intervals: [940e6, 931e6],
      sender: { bps: 935e6, bytes: 223 * 1024 ** 2, retransmits: 3 },
      receiver: { bps: 932e6, bytes: 222 * 1024 ** 2, retransmits: null },
    });
    expect(iperf3Flows(send.slice(0, 5), { direction: "receive", streams: 1 })).toEqual([
      { way: "down", intervals: [940e6, 931e6], sender: null, receiver: null },
    ]);
  });

  it("counts the sums of parallel streams, each way of a bidirectional test", () => {
    const both = [
      "[ ID][Role] Interval           Transfer     Bitrate         Retr  Cwnd",
      "[  5][TX-C]   0.00-1.00   sec  55.0 MBytes   461 Mbits/sec    0   1.00 MBytes       ",
      "[  7][TX-C]   0.00-1.00   sec  55.0 MBytes   461 Mbits/sec    0   1.00 MBytes       ",
      "[SUM][TX-C]   0.00-1.00   sec   110 MBytes   922 Mbits/sec    0             ",
      "[  9][RX-C]   0.00-1.00   sec  25.0 MBytes   210 Mbits/sec                  ",
      "[ 11][RX-C]   0.00-1.00   sec  25.0 MBytes   210 Mbits/sec                  ",
      "[SUM][RX-C]   0.00-1.00   sec  50.0 MBytes   420 Mbits/sec                  ",
      "[ ID][Role] Interval           Transfer     Bitrate         Retr",
      "[  5][TX-C]   0.00-1.00   sec  55.0 MBytes   461 Mbits/sec    0             sender",
      "[SUM][TX-C]   0.00-1.00   sec   110 MBytes   922 Mbits/sec    1             sender",
      "[SUM][TX-C]   0.00-1.00   sec   109 MBytes   915 Mbits/sec                  receiver",
      "[SUM][RX-C]   0.00-1.00   sec  50.0 MBytes   420 Mbits/sec    9             sender",
      "[SUM][RX-C]   0.00-1.00   sec  49.9 MBytes   1.2 Gbits/sec                  receiver",
    ];
    const flows = iperf3Flows(both, { direction: "both", streams: 2 });
    expect(flows.map((flow) => [flow.way, flow.intervals, flow.sender?.retransmits])).toEqual([
      ["up", [922e6], 1],
      ["down", [420e6], 9],
    ]);
    expect(flows[1]?.receiver?.bps).toBe(1.2e9);
  });
});

describe("a run's topic", () => {
  it("asks for a snapshot before a delta can apply", () => {
    const state = emptyRunView();
    expect(
      applyDiag(state, { type: "diag_delta", topic: "diag:7", lines: [], dropped: 0, end: null }),
    ).toBe(false);
    applyDiag(state, { type: "diag_snapshot", topic: "diag:7", run: run({ lines: [line("a")] }) });
    expect(state.ready).toBe(true);
    applyDiag(state, {
      type: "diag_delta",
      topic: "diag:7",
      lines: [line("b"), line("c")],
      dropped: 0,
      end: { state: "succeeded", ended_at: 2_000, exit_code: 0, error: null },
    });
    expect(state.run?.lines.map((l) => l.text)).toEqual(["a", "b", "c"]);
    expect(state.run).toMatchObject({ state: "succeeded", ended_at: 2_000, exit_code: 0 });
  });

  it("drops the lines the controller dropped, and keeps at most its own budget", () => {
    const state = emptyRunView();
    applyDiag(state, {
      type: "diag_snapshot",
      topic: "diag:7",
      run: run({ lines: [line("a"), line("b")] }),
    });
    applyDiag(state, {
      type: "diag_delta",
      topic: "diag:7",
      lines: [line("c")],
      dropped: 1,
      end: null,
    });
    expect(state.run?.lines.map((l) => l.text)).toEqual(["b", "c"]);
    expect(state.run?.dropped).toBe(1);
    const many = Array.from({ length: MAX_LINES }, (_, i) => line(String(i)));
    applyDiag(state, { type: "diag_delta", topic: "diag:7", lines: many, dropped: 1, end: null });
    expect(state.run?.lines.length).toBe(MAX_LINES);
    expect(state.run?.lines[0]?.text).toBe("0");
    expect(state.run?.dropped).toBe(3);
  });

  it("knows a run the controller forgot", () => {
    const state = emptyRunView();
    applyDiag(state, { type: "error", topic: "diag:7", message: "no such run" });
    expect(state).toMatchObject({ ready: true, missing: true });
  });
});

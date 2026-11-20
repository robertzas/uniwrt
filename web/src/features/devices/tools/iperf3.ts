import type { Iperf3Direction } from "../../../api/gen/Iperf3Direction";

/** Which way data flowed, seen from the run's node: `up` it sent, `down` it received. */
export type Way = "up" | "down";

/** One end's total, from its `sender` or `receiver` line. */
export interface Total {
  bps: number;
  bytes: number;
  /** The sender's retransmits, where iperf3 counts them. */
  retransmits: number | null;
}

/** One direction of a test: each second's rate as it passes, then both ends' totals. */
export interface Flow {
  way: Way;
  /** Bits per second, one per interval, oldest first. */
  intervals: number[];
  sender: Total | null;
  receiver: Total | null;
}

const SCALE: Record<string, number> = { "": 1, K: 1e3, M: 1e6, G: 1e9, T: 1e12 };
const BINARY: Record<string, number> = { "": 1, K: 1024, M: 1024 ** 2, G: 1024 ** 3, T: 1024 ** 4 };

/**
 * A line of iperf3's table: `[  5]`, `[SUM]`, or with `--bidir` `[  5][TX-C]`, the interval, the
 * transfer (binary units), the bitrate (decimal units), then retransmits, the window, or the role.
 */
const LINE =
  /^\[\s*(\d+|SUM)\](?:\[(TX|RX)-C\])?\s+[\d.]+-\s*[\d.]+\s+sec\s+([\d.]+)\s+([KMGT]?)Bytes\s+([\d.]+)\s+([KMGT]?)bits\/sec(.*)$/;

/**
 * The flows of an iperf3 client's text output (`-f m -i 1`): each second's rate, and the totals
 * once it prints them. With several streams only the sums count. `direction` places a flow
 * that `--bidir` doesn't tag.
 */
export function iperf3Flows(
  lines: readonly string[],
  test: { direction: Iperf3Direction; streams: number },
): Flow[] {
  const flows = new Map<Way, Flow>();
  const flow = (way: Way) => {
    let found = flows.get(way);
    if (!found) {
      found = { way, intervals: [], sender: null, receiver: null };
      flows.set(way, found);
    }
    return found;
  };
  for (const line of lines) {
    const match = LINE.exec(line.trimEnd());
    if (!match) continue;
    const [, id, tag, transfer, transferUnit, rate, rateUnit, rest] = match;
    if ((id === "SUM") !== test.streams > 1) continue;
    const way: Way = tag
      ? tag === "TX"
        ? "up"
        : "down"
      : test.direction === "receive"
        ? "down"
        : "up";
    const bps = Number(rate) * (SCALE[rateUnit ?? ""] ?? 1);
    const tail = (rest ?? "").trim();
    if (tail.endsWith("sender") || tail.endsWith("receiver")) {
      const retr = /^(\d+)\s+sender$/.exec(tail);
      const total: Total = {
        bps,
        bytes: Number(transfer) * (BINARY[transferUnit ?? ""] ?? 1),
        retransmits: retr ? Number(retr[1]) : null,
      };
      if (tail.endsWith("sender")) flow(way).sender = total;
      else flow(way).receiver = total;
    } else {
      flow(way).intervals.push(bps);
    }
  }
  return (["up", "down"] as const).flatMap((way) => flows.get(way) ?? []);
}

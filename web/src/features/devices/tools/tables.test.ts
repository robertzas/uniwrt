import { describe, expect, it } from "vitest";
import type { TrackedConnection } from "../../../api/gen/TrackedConnection";
import { client } from "../../../test/clients";
import { pcap } from "../../../test/pcap";
import { captureErrors, emptyCapture, filterError, PcapCounter, toCaptureQuery } from "./capture";
import {
  compareAddresses,
  endpoint,
  filterWords,
  indexClients,
  matchesWords,
  natOf,
  neighborClient,
  trafficOf,
} from "./tables";

function connection(extra: Partial<TrackedConnection> = {}): TrackedConnection {
  return {
    family: "ipv4",
    protocol: "tcp",
    protocol_number: 6,
    state: "ESTABLISHED",
    expires_s: 7200,
    original: {
      src: "192.168.1.20",
      dst: "203.0.113.5",
      sport: 51515,
      dport: 443,
      packets: 10,
      bytes: 1000,
    },
    reply: {
      src: "203.0.113.5",
      dst: "192.168.1.20",
      sport: 443,
      dport: 51515,
      packets: 12,
      bytes: 9000,
    },
    assured: true,
    unreplied: false,
    offloaded: false,
    mark: 0,
    ...extra,
  };
}

describe("the device's tables", () => {
  it("orders addresses and prefixes by value", () => {
    const sorted = [
      "fe80::1",
      "192.168.1.10",
      "10.0.0.0/8",
      "192.168.1.9",
      "default",
      "2001:db8::/32",
      "::ffff:192.0.2.1",
      "192.168.1.0/24",
      "local-name",
    ].sort(compareAddresses);
    expect(sorted).toEqual([
      "default",
      "10.0.0.0/8",
      "192.168.1.0/24",
      "192.168.1.9",
      "192.168.1.10",
      "::ffff:192.0.2.1",
      "2001:db8::/32",
      "fe80::1",
      "local-name",
    ]);
  });

  it("filters by every word, in any field", () => {
    const words = filterWords("  LAN  reach ");
    expect(words).toEqual(["lan", "reach"]);
    expect(matchesWords(["br-lan", "REACHABLE", null], words)).toBe(true);
    expect(matchesWords(["br-lan", "STALE"], words)).toBe(false);
    expect(matchesWords(["anything"], [])).toBe(true);
  });

  it("finds a neighbor's client by MAC, else by address", () => {
    const index = indexClients([
      client("02:00:00:00:00:01", { ips: ["192.168.1.20"], alias: "laptop" }),
      client("02:00:00:00:00:02", { ips: ["192.168.1.30"], online: false }),
      client("02:00:00:00:00:03", { ips: ["192.168.1.30"] }),
    ]);
    const entry = {
      address: "192.168.1.99",
      mac: "02:00:00:00:00:01",
      device: "br-lan",
      state: null,
      router: false,
    };
    expect(neighborClient(entry, index)?.alias).toBe("laptop");
    expect(neighborClient({ ...entry, mac: null, address: "192.168.1.30" }, index)?.mac).toBe(
      "02:00:00:00:00:03",
    );
    expect(neighborClient({ ...entry, mac: "02:00:00:00:00:09" }, index)).toBeUndefined();
  });

  it("shows where NAT moved a connection, and its traffic", () => {
    // Masqueraded out of the WAN: the reply comes back to the WAN address.
    const out = connection({
      reply: {
        src: "203.0.113.5",
        dst: "198.51.100.7",
        sport: 443,
        dport: 61000,
        packets: 1,
        bytes: 100,
      },
    });
    expect(natOf(out)).toEqual({ snat: "198.51.100.7:61000" });
    // A port forward: the first packet asked for the WAN, the LAN host answered.
    const forward = connection({
      original: {
        src: "203.0.113.9",
        dst: "198.51.100.7",
        sport: 4000,
        dport: 8443,
        packets: null,
        bytes: null,
      },
      reply: {
        src: "192.168.1.20",
        dst: "203.0.113.9",
        sport: 443,
        dport: 4000,
        packets: null,
        bytes: null,
      },
    });
    expect(natOf(forward)).toEqual({ dnat: "192.168.1.20:443" });
    expect(trafficOf(forward)).toBeNull();
    expect(natOf(connection())).toEqual({});
    expect(trafficOf(connection())).toBe(10_000);
    expect(endpoint("2001:db8::1", 443)).toBe("[2001:db8::1]:443");
    expect(endpoint("192.0.2.1", null)).toBe("192.0.2.1");
  });
});

describe("a packet capture", () => {
  it("checks the form as the controller does", () => {
    expect(captureErrors(emptyCapture())).toEqual({});
    expect(filterError("-w /tmp/x")).toBe('A filter can\'t start with "-".');
    expect(filterError("port 53)")).toBe("The filter closes a parenthesis it never opened.");
    expect(filterError("(port 53")).toBe("The filter leaves a parenthesis open.");
    expect(filterError("port\t53")).toBe("A filter is printable ASCII on one line.");
    expect(filterError("x".repeat(1025))).toBe("A filter is at most 1,024 characters long.");
    expect(filterError("(tcp port 80) or udp")).toBeUndefined();
    const bad = { interface: "br lan", filter: "", seconds: "0", maxMib: "257", snaplen: "64" };
    expect(Object.keys(captureErrors(bad)).sort()).toEqual([
      "interface",
      "maxMib",
      "seconds",
      "snaplen",
    ]);
    expect(toCaptureQuery(bad)).toBeUndefined();
    expect(
      toCaptureQuery({
        ...emptyCapture(),
        interface: "br-lan",
        filter: " port 53 ",
        snaplen: "128",
      }),
    ).toEqual({ interface: "br-lan", filter: "port 53", seconds: 60, max_mib: 16, snaplen: 128 });
  });

  it("counts whole records across chunks, in either byte order", () => {
    for (const little of [true, false]) {
      const file = pcap([60, 1500, 42], little);
      const counter = new PcapCounter();
      // Split mid-header and mid-record.
      for (const [from, to] of [
        [0, 10],
        [10, 30],
        [30, 1000],
        [1000, file.length - 5],
      ]) {
        counter.push(file.subarray(from, to));
      }
      expect(counter.packets).toBe(2);
      expect(counter.complete).toBe(24 + 16 + 60 + 16 + 1500);
      counter.push(file.subarray(file.length - 5));
      expect(counter.packets).toBe(3);
      expect(counter.complete).toBe(file.length);
    }
  });

  it("keeps all of a stream that isn't a pcap", () => {
    const counter = new PcapCounter();
    counter.push(new TextEncoder().encode("not a capture file at all, really"));
    expect(counter.packets).toBe(0);
    expect(counter.complete).toBe(33);
  });
});

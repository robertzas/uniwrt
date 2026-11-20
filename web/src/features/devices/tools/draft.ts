import type { DiagTool } from "../../../api/gen/DiagTool";
import type { DnsRecordType } from "../../../api/gen/DnsRecordType";
import type { Iperf3Direction } from "../../../api/gen/Iperf3Direction";
import type { IpFamily } from "../../../api/gen/IpFamily";
import type { OptionalTool } from "../../../api/gen/OptionalTool";
import { ipv4 } from "../../../lib/ipv4";
import { strings } from "../../../lib/strings";

const e = strings.device.tools.errors;

/** The tools a run can be, in the order the form offers them. */
export type ToolKind = DiagTool["tool"];
export const TOOL_KINDS: readonly ToolKind[] = [
  "ping",
  "traceroute",
  "dns",
  "http",
  "iperf3",
  "cable_test",
];

/** The model's limits (`uniwrt_model::diag`). */
export const MAX_PING_COUNT = 100;
export const MAX_IPERF3_SECONDS = 60;
export const MAX_IPERF3_STREAMS = 8;
const MAX_TARGET_LEN = 253;
const MAX_URL_LEN = 2048;

export const RECORD_TYPES: readonly DnsRecordType[] = [
  "A",
  "AAAA",
  "CNAME",
  "MX",
  "NS",
  "PTR",
  "SOA",
  "SRV",
  "TXT",
];

/** The form's fields, as typed: every tool's, so switching tools keeps what was entered. */
export interface Draft {
  tool: ToolKind;
  /** Ping's and traceroute's host. */
  target: string;
  family: IpFamily | "";
  count: string;
  /** The DNS lookup's name, its record type (both A and AAAA when empty) and server. */
  name: string;
  record: DnsRecordType | "";
  server: string;
  url: string;
  /** The cable test's port. */
  port: string;
  /** `controller`, or the far node's id. */
  peer: string;
  direction: Iperf3Direction;
  seconds: string;
  streams: string;
}

export function emptyDraft(): Draft {
  return {
    tool: "ping",
    target: "",
    family: "",
    count: "10",
    name: "",
    record: "",
    server: "",
    url: "https://",
    port: "",
    peer: "controller",
    direction: "send",
    seconds: "10",
    streams: "1",
  };
}

/** The optional tool a run needs (`DiagTool::needs`). */
export function needs(tool: ToolKind): OptionalTool | undefined {
  if (tool === "iperf3") return "iperf3";
  if (tool === "cable_test") return "ethtool";
  return undefined;
}

function isIpv6(text: string): boolean {
  if (!text.includes(":") || !/^[0-9a-fA-F:.]+$/.test(text)) return false;
  try {
    new URL(`http://[${text}]/`);
    return true;
  } catch {
    return false;
  }
}

/** An IPv4 or IPv6 address, as Rust's `IpAddr` parses one. */
export function isAddress(text: string): boolean {
  return (/^\d+\.\d+\.\d+\.\d+$/.test(text) && ipv4(text) !== undefined) || isIpv6(text);
}

/**
 * A name a resolver may be asked about: dot-separated labels of letters, digits, `-` and `_`, none
 * starting with `-`, with an optional final dot (the model's `is_lookup_name`).
 */
export function isLookupName(text: string): boolean {
  const name = text.endsWith(".") ? text.slice(0, -1) : text;
  return (
    name.length > 0 &&
    name.length <= MAX_TARGET_LEN &&
    name
      .split(".")
      .every(
        (label) =>
          label.length > 0 && label.length <= 63 && /^[A-Za-z0-9_][A-Za-z0-9_-]*$/.test(label),
      )
  );
}

/** A network device's name as Linux takes it (the model's `is_ifname`). */
export function isIfname(text: string): boolean {
  return text.length <= 15 && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(text);
}

/** An `http://` or `https://` URL with a host, of printable ASCII. */
export function isHttpUrl(text: string): boolean {
  if (text.length > MAX_URL_LEN || !/^[\x21-\x7e]+$/.test(text)) return false;
  const rest = text.startsWith("http://")
    ? text.slice(7)
    : text.startsWith("https://")
      ? text.slice(8)
      : undefined;
  return rest !== undefined && (rest.split(/[/?#]/)[0] ?? "").length > 0;
}

function whole(text: string, min: number, max: number): number | undefined {
  if (!/^\d+$/.test(text.trim())) return undefined;
  const value = Number(text.trim());
  return value >= min && value <= max ? value : undefined;
}

/** What is wrong with each field the draft's tool uses; empty when it can run. */
export type Errors = Partial<Record<keyof Draft, string>>;

export function draftErrors(draft: Draft): Errors {
  const errors: Errors = {};
  const host = (field: "target", text: string) => {
    if (!isAddress(text) && !isLookupName(text)) errors[field] = e.host;
  };
  switch (draft.tool) {
    case "ping":
      host("target", draft.target.trim());
      if (whole(draft.count, 1, MAX_PING_COUNT) === undefined) errors.count = e.count;
      break;
    case "traceroute":
      host("target", draft.target.trim());
      break;
    case "dns": {
      const name = draft.name.trim();
      if (!isLookupName(name) && !isAddress(name)) errors.name = e.name;
      const server = draft.server.trim();
      if (server && !isAddress(server) && !isLookupName(server)) errors.server = e.server;
      break;
    }
    case "http":
      if (!isHttpUrl(draft.url.trim())) errors.url = e.url;
      break;
    case "cable_test":
      if (!isIfname(draft.port.trim())) errors.port = e.port;
      break;
    case "iperf3":
      if (draft.peer !== "controller" && !/^\d+$/.test(draft.peer)) errors.peer = e.peer;
      if (whole(draft.seconds, 1, MAX_IPERF3_SECONDS) === undefined) errors.seconds = e.seconds;
      if (whole(draft.streams, 1, MAX_IPERF3_STREAMS) === undefined) errors.streams = e.streams;
      break;
  }
  return errors;
}

/** The tool the draft asks for, or undefined while a field it uses is wrong. */
export function toolOf(draft: Draft): DiagTool | undefined {
  if (Object.keys(draftErrors(draft)).length > 0) return undefined;
  const family = draft.family === "" ? null : draft.family;
  switch (draft.tool) {
    case "ping":
      return {
        tool: "ping",
        target: draft.target.trim(),
        family,
        count: Number(draft.count.trim()),
      };
    case "traceroute":
      return { tool: "traceroute", target: draft.target.trim(), family };
    case "dns":
      return {
        tool: "dns",
        name: draft.name.trim(),
        record: draft.record === "" ? null : draft.record,
        server: draft.server.trim() || null,
      };
    case "http":
      return { tool: "http", url: draft.url.trim() };
    case "cable_test":
      return { tool: "cable_test", port: draft.port.trim() };
    case "iperf3":
      return {
        tool: "iperf3",
        peer: draft.peer === "controller" ? "controller" : { node: Number(draft.peer) },
        direction: draft.direction,
        seconds: Number(draft.seconds.trim()),
        streams: Number(draft.streams.trim()),
      };
  }
}

/** A run's tool in a few words, such as `Ping 1.1.1.1`; `name` names a peer node. */
export function describeTool(tool: DiagTool, name: (node: number) => string): string {
  const s = strings.device.tools.summary;
  switch (tool.tool) {
    case "ping":
      return s.ping(tool.target);
    case "traceroute":
      return s.traceroute(tool.target);
    case "dns":
      return s.dns(tool.name, tool.record);
    case "http":
      return s.http(tool.url);
    case "cable_test":
      return s.cable(tool.port);
    case "iperf3":
      return s.iperf3(
        tool.peer === "controller" ? s.controller : name(tool.peer.node),
        tool.direction,
      );
  }
}

/** The ports a cable test can pick: the board's LAN ports and its WAN, as Linux names them. */
export function cablePorts(layout: { lan: readonly string[]; wan: string | null } | undefined) {
  const names = [...(layout?.lan ?? []), ...(layout?.wan ? [layout.wan] : [])];
  return [...new Set(names)].filter(isIfname);
}

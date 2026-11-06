import type { Ipv4Kind } from "../../api/gen/Ipv4Kind";
import type { Preset } from "../../api/gen/Preset";
import type { Prompt } from "../../api/gen/Prompt";
import type { WanConfig } from "../../api/gen/WanConfig";
import type { WanIpv6 } from "../../api/gen/WanIpv6";
import type { WanProto } from "../../api/gen/WanProto";

/** How IPv4 arrives, as the site names it. */
export type Proto = WanProto["kind"];
/** How IPv6 arrives. */
export type Ipv6Mode = WanIpv6["mode"];

/**
 * The WAN form (wan.md, Wizard flow): every field as typed. A preset fills it; the advanced
 * fields stay editable. `password` is a new value; empty keeps the stored one.
 */
export interface Draft {
  preset: string | null;
  proto: Proto;
  vlan: string;
  pcp: string;
  mtu: string;
  mac: string;
  vendorClass: string;
  clientId: string;
  /** DHCP options, `<code>:<value>` separated by spaces. */
  sendOptions: string;
  hostname: string;
  address: string;
  gateway: string;
  /** DNS servers, separated by spaces or commas. */
  dns: string;
  username: string;
  password: string;
  service: string;
  aftr: string;
  ipv6: Ipv6Mode;
  reqprefix: string;
  sixrdPrefix: string;
  borderRelay: string;
  ipv4MaskLen: string;
}

/** The fields, as the form names them. */
export type Field = Exclude<keyof Draft, "preset" | "proto" | "ipv6">;

const text = (value: string | number | null | undefined) =>
  value === null || value === undefined ? "" : String(value);

const PROTO_OF: Record<Ipv4Kind, Proto> = {
  dhcp: "dhcp",
  pppoe: "pppoe",
  static: "static",
  dslite: "ds_lite",
  none: "none",
};

/** The form for the site's WAN. */
export function draftOf(wan: WanConfig): Draft {
  const p = wan.proto;
  const v6 = wan.ipv6;
  return {
    preset: wan.preset,
    proto: p.kind,
    vlan: text(wan.vlan),
    pcp: text(wan.pcp),
    mtu: text(wan.mtu),
    mac: text(wan.mac),
    vendorClass: p.kind === "dhcp" ? text(p.vendor_class) : "",
    clientId: p.kind === "dhcp" ? text(p.client_id) : "",
    sendOptions: p.kind === "dhcp" ? p.send_options.join(" ") : "",
    hostname: p.kind === "dhcp" ? text(p.hostname) : "",
    address: p.kind === "static" ? p.address : "",
    gateway: p.kind === "static" ? p.gateway : "",
    dns: p.kind === "static" ? p.dns.join(", ") : "",
    username: p.kind === "pppoe" ? p.username : "",
    password: "",
    service: p.kind === "pppoe" ? text(p.service) : "",
    aftr: p.kind === "ds_lite" ? text(p.aftr) : "",
    ipv6: v6.mode,
    reqprefix: v6.mode === "dhcpv6" ? text(v6.reqprefix) : "",
    sixrdPrefix: v6.mode === "6rd" ? v6.prefix : "",
    borderRelay: v6.mode === "6rd" ? v6.border_relay : "",
    ipv4MaskLen: v6.mode === "6rd" ? String(v6.ipv4_mask_len) : "",
  };
}

/**
 * The form filled from `preset`: its values replace the connection's, while what the user
 * typed for prompts (credentials, addresses, the MAC) stays. IPv6 `auto` starts with DHCPv6;
 * the test tries the preset's 6rd after ([`fallbackOf`]).
 */
export function withPreset(from: Draft, preset: Preset): Draft {
  const v4 = preset.ipv4;
  const v6 = preset.ipv6;
  const dhcp = v4.dhcp;
  const sixrd = v6.sixrd;
  const prompted = new Set<Prompt>(preset.prompts);
  return {
    ...from,
    preset: preset.id,
    proto: PROTO_OF[v4.proto],
    vlan: prompted.has("vlan") ? from.vlan : text(v4.vlan),
    pcp: text(v4.pcp),
    mtu: text(v4.mtu),
    mac: prompted.has("mac") ? from.mac : "",
    vendorClass: text(dhcp?.vendor_class),
    clientId: text(dhcp?.client_id),
    sendOptions: dhcp ? dhcp.send_options.join(" ") : "",
    hostname: text(dhcp?.hostname),
    service: text(v4.service),
    aftr: text(v4.aftr),
    ipv6: v6.mode === "auto" ? "dhcpv6" : v6.mode,
    reqprefix: text(v6.reqprefix),
    sixrdPrefix: sixrd && v6.mode === "6rd" ? sixrd.prefix : from.sixrdPrefix,
    borderRelay: sixrd && v6.mode === "6rd" ? sixrd.border_relay : from.borderRelay,
    ipv4MaskLen: sixrd && v6.mode === "6rd" ? String(sixrd.ipv4_mask_len) : from.ipv4MaskLen,
  };
}

/** The IPv6 the test tries when DHCPv6 delegates no prefix: an `auto` preset's 6rd. */
export function fallbackOf(preset: Preset | undefined): WanIpv6 | undefined {
  const sixrd = preset?.ipv6.sixrd;
  if (preset?.ipv6.mode !== "auto" || !sixrd) return undefined;
  return { mode: "6rd", ...sixrd };
}

/** The fields a preset asks for, in the order it asks (the wizard's first view). */
export function promptedFields(preset: Preset): Field[] {
  const fields: Record<Prompt, Field[]> = {
    username: ["username"],
    password: ["password"],
    address: ["address"],
    gateway: ["gateway"],
    dns: ["dns"],
    vlan: ["vlan"],
    mac: ["mac"],
    aftr: ["aftr"],
    sixrd: ["sixrdPrefix", "borderRelay", "ipv4MaskLen"],
  };
  return preset.prompts.flatMap((prompt) => fields[prompt]);
}

/** Every field that applies to the form's connection (the advanced view). */
export function fieldsOf(draft: Draft): Field[] {
  const byProto: Record<Proto, Field[]> = {
    dhcp: ["vendorClass", "clientId", "sendOptions", "hostname"],
    static: ["address", "gateway", "dns"],
    pppoe: ["username", "password", "service"],
    ds_lite: ["aftr"],
    none: [],
  };
  const byIpv6: Record<Ipv6Mode, Field[]> = {
    off: [],
    dhcpv6: ["reqprefix"],
    "6rd": ["sixrdPrefix", "borderRelay", "ipv4MaskLen"],
  };
  return [...byProto[draft.proto], "vlan", "pcp", "mtu", "mac", ...byIpv6[draft.ipv6]];
}

const optional = (value: string) => (value.trim() === "" ? null : value.trim());
const number = (value: string) => (value.trim() === "" ? null : Number(value.trim()));
const list = (value: string) =>
  value
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter((item) => item !== "");

/**
 * The site's WAN from a form [`check`]ed first. `current` lends its PPPoE password reference
 * when none is typed; a typed one goes with the save (or the test) as a secret.
 */
export function wanOf(draft: Draft, current: WanConfig): WanConfig {
  const stored = current.proto.kind === "pppoe" ? current.proto.password : null;
  const proto: WanProto = (() => {
    switch (draft.proto) {
      case "dhcp":
        return {
          kind: "dhcp",
          vendor_class: optional(draft.vendorClass),
          client_id: optional(draft.clientId),
          send_options: list(draft.sendOptions),
          hostname: optional(draft.hostname),
        };
      case "static":
        return {
          kind: "static",
          address: draft.address.trim(),
          gateway: draft.gateway.trim(),
          dns: list(draft.dns),
        };
      case "pppoe":
        return {
          kind: "pppoe",
          username: draft.username.trim(),
          password: draft.password === "" ? stored : null,
          service: optional(draft.service),
        };
      case "ds_lite":
        return { kind: "ds_lite", aftr: optional(draft.aftr) };
      case "none":
        return { kind: "none" };
    }
  })();
  const ipv6: WanIpv6 = (() => {
    switch (draft.ipv6) {
      case "off":
        return { mode: "off" };
      case "dhcpv6":
        return { mode: "dhcpv6", reqprefix: number(draft.reqprefix) };
      case "6rd":
        return {
          mode: "6rd",
          prefix: draft.sixrdPrefix.trim(),
          border_relay: draft.borderRelay.trim(),
          ipv4_mask_len: number(draft.ipv4MaskLen) ?? 0,
        };
    }
  })();
  return {
    proto,
    vlan: number(draft.vlan),
    pcp: number(draft.pcp),
    mtu: number(draft.mtu),
    mac: optional(draft.mac)?.toLowerCase() ?? null,
    ipv6,
    preset: draft.preset,
  };
}

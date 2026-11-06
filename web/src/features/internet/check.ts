import { strings } from "../../lib/strings";
import type { Draft, Field } from "./draft";

const e = strings.internet.errors;

/** What is wrong with each field, as the server checks it (crates/model, validate/gateway.rs). */
export type DraftErrors = Partial<Record<Field, string>>;

const IPV4 = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$/;
const IPV6 = /^[0-9a-f:]+(%\w+)?$/i;

export const isIpv4 = (value: string) => IPV4.test(value);
const isIpv6 = (value: string) => value.includes(":") && IPV6.test(value);
const isIp = (value: string) => isIpv4(value) || isIpv6(value);

/** An IPv4 address with its prefix, such as `203.0.113.10/29`. */
function cidr(value: string): { addr: number; len: number } | undefined {
  const [ip, len] = value.split("/");
  if (!ip || !isIpv4(ip) || len === undefined || !/^\d{1,2}$/.test(len) || Number(len) > 32) {
    return undefined;
  }
  const addr = ip.split(".").reduce((n, octet) => n * 256 + Number(octet), 0);
  return { addr, len: Number(len) };
}

const toInt = (ip: string) => ip.split(".").reduce((n, octet) => n * 256 + Number(octet), 0);

function range(value: string, low: number, high: number): boolean {
  return /^\d+$/.test(value.trim()) && Number(value) >= low && Number(value) <= high;
}

const printable = (value: string) => !/\p{Cc}/u.test(value);

/** The form's errors; `storedPassword` says a PPPoE password is set already. */
export function check(draft: Draft, storedPassword: boolean): DraftErrors {
  const errors: DraftErrors = {};
  const set = (field: Field, message: string) => {
    errors[field] ??= message;
  };
  const vlan = draft.vlan.trim();
  if (vlan !== "" && !range(vlan, 1, 4094)) set("vlan", e.vlan);
  if (draft.pcp.trim() !== "") {
    if (!range(draft.pcp, 0, 7)) set("pcp", e.pcp);
    else if (vlan === "") set("pcp", e.pcpVlan);
  }
  if (draft.mtu.trim() !== "") {
    const least = draft.ipv6 === "off" ? 576 : 1280;
    if (!range(draft.mtu, least, 9000)) set("mtu", e.mtu(least));
  }
  const mac = draft.mac.trim();
  if (mac !== "" && !/^([0-9a-f]{2}[:-]){5}[0-9a-f]{2}$/i.test(mac)) set("mac", e.mac);

  switch (draft.proto) {
    case "dhcp": {
      const id = draft.clientId.trim();
      if (id !== "" && !/^([0-9a-f]{2})+$/i.test(id)) set("clientId", e.clientId);
      for (const option of draft.sendOptions.split(/\s+/).filter((o) => o !== "")) {
        const [code, ...value] = option.split(":");
        if (!range(code ?? "", 1, 254) || value.join(":") === "") set("sendOptions", e.sendOption);
      }
      if (!printable(draft.vendorClass)) set("vendorClass", e.printable);
      if (!printable(draft.hostname)) set("hostname", e.printable);
      break;
    }
    case "static": {
      const net = cidr(draft.address.trim());
      if (!net || net.len > 30) {
        set("address", e.address);
      } else {
        const size = 2 ** (32 - net.len);
        const network = net.addr - (net.addr % size);
        if (net.addr === network || net.addr === network + size - 1) set("address", e.host);
        const gw = draft.gateway.trim();
        if (!isIpv4(gw)) set("gateway", e.ip);
        else if (toInt(gw) - network < 0 || toInt(gw) - network >= size || toInt(gw) === net.addr) {
          set("gateway", e.gateway);
        }
      }
      const dns = draft.dns.split(/[\s,]+/).filter((d) => d !== "");
      if (dns.some((d) => !isIp(d))) set("dns", e.ip);
      break;
    }
    case "pppoe":
      if (draft.username.trim() === "") set("username", e.username);
      else if (!printable(draft.username)) set("username", e.printable);
      if (draft.password === "" && !storedPassword) set("password", e.password);
      else if (draft.password.length > 128 || !printable(draft.password)) {
        set("password", e.passwordForm);
      }
      if (!printable(draft.service)) set("service", e.printable);
      break;
    case "ds_lite":
      if (/\s/.test(draft.aftr.trim())) set("aftr", e.aftr);
      break;
    case "none":
      break;
  }
  if (draft.ipv6 === "dhcpv6" && draft.reqprefix.trim() !== "" && !range(draft.reqprefix, 48, 64)) {
    set("reqprefix", e.reqprefix);
  }
  if (draft.ipv6 === "6rd") {
    const [prefix, len] = draft.sixrdPrefix.trim().split("/");
    const mask = draft.ipv4MaskLen.trim() === "" ? "0" : draft.ipv4MaskLen;
    if (!prefix || !isIpv6(prefix) || !range(len ?? "", 1, 64)) set("sixrdPrefix", e.prefix);
    else if (range(mask, 0, 32) && Number(len) + 32 - Number(mask) > 64) {
      set("sixrdPrefix", e.prefixBits);
    }
    if (!range(mask, 0, 32)) set("ipv4MaskLen", e.mask);
    if (!isIpv4(draft.borderRelay.trim())) set("borderRelay", e.ip);
  }
  return errors;
}

/** Why the connection and IPv6 settings can't go together, if they can't. */
export function comboError(draft: Draft): string | undefined {
  if ((draft.proto === "ds_lite" || draft.proto === "none") && draft.ipv6 !== "dhcpv6") {
    return draft.proto === "ds_lite" ? e.dsliteIpv6 : e.noneIpv6;
  }
  return undefined;
}

const PATHS: Record<string, Field> = {
  "wan/vlan": "vlan",
  "wan/pcp": "pcp",
  "wan/mtu": "mtu",
  "wan/mac": "mac",
  "wan/password": "password",
  "wan/proto/address": "address",
  "wan/proto/gateway": "gateway",
  "wan/proto/dns": "dns",
  "wan/proto/username": "username",
  "wan/proto/service": "service",
  "wan/proto/aftr": "aftr",
  "wan/proto/vendor_class": "vendorClass",
  "wan/proto/client_id": "clientId",
  "wan/proto/hostname": "hostname",
  "wan/ipv6/reqprefix": "reqprefix",
  "wan/ipv6/prefix": "sixrdPrefix",
  "wan/ipv6/border_relay": "borderRelay",
  "wan/ipv6/ipv4_mask_len": "ipv4MaskLen",
};

/** The server's issues (422) as the form's errors; those of no field (`wan/ipv6`) apart. */
export function errorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: DraftErrors;
  other: string[];
} {
  const errors: DraftErrors = {};
  const other: string[] = [];
  for (const issue of issues) {
    const path = issue.path.startsWith("wan/proto/send_options/")
      ? "wan/proto/send_options"
      : issue.path;
    const field = path === "wan/proto/send_options" ? "sendOptions" : PATHS[path];
    if (field) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

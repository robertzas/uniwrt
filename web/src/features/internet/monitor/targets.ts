/**
 * The WAN monitor's targets as their editor holds them, checked as the model checks them
 * (`uniwrt_model::wan_monitor`, `WanMonitorSettings::validate`).
 */
import type { WanDnsVia } from "../../../api/gen/WanDnsVia";
import type { WanMonitorSettings } from "../../../api/gen/WanMonitorSettings";
import type { WanProbe } from "../../../api/gen/WanProbe";
import type { WanProbeTarget } from "../../../api/gen/WanProbeTarget";
import { strings } from "../../../lib/strings";

const e = strings.internet.monitor.editor.errors;

export const TARGETS_MAX = 12;
const ID_MAX = 24;
const NAME_MAX = 48;
const URL_MAX = 255;

export type ProbeKind = WanProbe["kind"];
export type ViaKind = WanDnsVia["via"];
export const PROBE_KINDS: readonly ProbeKind[] = ["icmp", "dns", "http"];
export const VIA_KINDS: readonly ViaKind[] = ["gateway", "upstreams", "server"];

/** A target as typed: every kind's fields, so switching kind keeps what was typed. */
export interface TargetDraft {
  /** A saved target's id, kept so its series and alert rules go on; `null` for a new one. */
  id: string | null;
  name: string;
  kind: ProbeKind;
  host: string;
  lookup: string;
  via: ViaKind;
  server: string;
  url: string;
}

export type TargetErrors = Partial<Record<"name" | "host" | "lookup" | "server" | "url", string>>;

export function newDraft(): TargetDraft {
  return {
    id: null,
    name: "",
    kind: "icmp",
    host: "",
    lookup: "openwrt.org",
    via: "gateway",
    server: "",
    url: "",
  };
}

export function draftOf(target: WanProbeTarget): TargetDraft {
  const draft = { ...newDraft(), id: target.id, name: target.name, kind: target.probe.kind };
  const probe = target.probe;
  if (probe.kind === "icmp") draft.host = probe.host ?? "";
  if (probe.kind === "dns") {
    draft.lookup = probe.name;
    draft.via = probe.via.via;
    if (probe.via.via === "server") draft.server = probe.via.address;
  }
  if (probe.kind === "http") draft.url = probe.url;
  return draft;
}

/** A DNS name as the model's `is_host_name`: labels of letters, digits and inner dashes. */
export function isHostName(text: string): boolean {
  const name = text.endsWith(".") ? text.slice(0, -1) : text;
  return (
    name !== "" &&
    name.length <= 253 &&
    name
      .split(".")
      .every((label) => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label)) &&
    !/^[0-9.]+$/.test(name)
  );
}

const OCTET = "(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)";
const IPV4 = new RegExp(`^${OCTET}(?:\\.${OCTET}){3}$`);

/** An IPv4 or IPv6 address, without a zone or brackets. */
export function isIp(text: string): boolean {
  if (IPV4.test(text)) return true;
  if (!text.includes(":") || !/^[0-9A-Fa-f:.]+$/.test(text)) return false;
  try {
    new URL(`http://[${text}]/`);
    return true;
  } catch {
    return false;
  }
}

/** An `http://` or `https://` URL whose host is a name or an address, as the model checks it. */
export function isProbeUrl(url: string): boolean {
  const rest = url.startsWith("http://")
    ? url.slice(7)
    : url.startsWith("https://")
      ? url.slice(8)
      : undefined;
  if (rest === undefined || url.length > URL_MAX || /[\s\p{Cc}]/u.test(url)) return false;
  const end = rest.search(/[/?#]/);
  const authority = end < 0 ? rest : rest.slice(0, end);
  const colon = authority.lastIndexOf(":");
  const port = colon < 0 ? "" : authority.slice(colon + 1);
  const host =
    colon >= 0 && /^\d{1,5}$/.test(port) && Number(port) <= 65_535
      ? authority.slice(0, colon)
      : authority;
  const bracketed = host.startsWith("[") && host.endsWith("]") ? host.slice(1, -1) : undefined;
  return isHostName(host) || isIp(host) || (bracketed !== undefined && isIp(bracketed));
}

/** An id from a name: `Cloudflare DNS` → `cloudflare-dns`. */
export function slug(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+/, "")
    .slice(0, ID_MAX)
    .replace(/-+$/, "");
}

/** `base`, or `base-2`, `base-3`… whichever `taken` doesn't hold, within the id's length. */
function unique(base: string, taken: Set<string>): string {
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    const suffix = `-${n}`;
    const id = `${base.slice(0, ID_MAX - suffix.length).replace(/-+$/, "")}${suffix}`;
    if (!taken.has(id)) return id;
  }
}

function probeOf(draft: TargetDraft, errors: TargetErrors): WanProbe {
  switch (draft.kind) {
    case "icmp": {
      const host = draft.host.trim();
      if (host !== "" && !isIp(host) && !isHostName(host)) errors.host = e.host;
      return { kind: "icmp", host: host === "" ? null : host };
    }
    case "dns": {
      const name = draft.lookup.trim();
      if (!isHostName(name)) errors.lookup = e.lookup;
      const server = draft.server.trim();
      if (draft.via === "server" && (!isIp(server) || /^(0\.0\.0\.0|::)$/.test(server))) {
        errors.server = e.server;
      }
      const via: WanDnsVia =
        draft.via === "server" ? { via: "server", address: server } : { via: draft.via };
      return { kind: "dns", via, name };
    }
    case "http": {
      const url = draft.url.trim();
      if (!isProbeUrl(url)) errors.url = e.url;
      return { kind: "http", url };
    }
  }
}

/**
 * The settings to save, or each target's errors (in the drafts' order). New targets take an id
 * from their name that no other target has.
 */
export function checkTargets(
  enabled: boolean,
  drafts: readonly TargetDraft[],
): { settings: WanMonitorSettings } | { errors: TargetErrors[] } {
  const taken = new Set(drafts.flatMap((draft) => (draft.id === null ? [] : [draft.id])));
  const all: TargetErrors[] = [];
  const targets: WanProbeTarget[] = drafts.map((draft) => {
    const errors: TargetErrors = {};
    const name = draft.name.trim();
    if (name === "" || [...name].length > NAME_MAX || /\p{Cc}/u.test(name)) errors.name = e.name;
    let id = draft.id;
    if (id === null) {
      const base = slug(name);
      if (base === "") errors.name ??= e.id;
      id = unique(base, taken);
      taken.add(id);
    }
    const probe = probeOf(draft, errors);
    all.push(errors);
    return { id, name, probe };
  });
  if (all.some((errors) => Object.keys(errors).length > 0)) return { errors: all };
  return { settings: { enabled, targets } };
}

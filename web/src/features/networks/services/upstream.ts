import type { DohResolver } from "../../../api/gen/DohResolver";
import type { Upstream } from "../../../api/gen/Upstream";
import { ipv4 } from "../../../lib/ipv4";
import { isDnsName } from "../../../lib/names";
import { strings } from "../../../lib/strings";
import { isIpv6 } from "../dns";

const e = strings.networks.upstream.errors;

/** The most servers, resolvers and bootstrap servers (`MAX_SERVERS` in the model). */
export const MAX = 4;

/** The card's three ways to send queries; layers and DoT come with M9.0b2's editor (D-146). */
export type Mode = "isp" | "custom" | "doh";
export const MODES: readonly Mode[] = ["isp", "custom", "doh"];

/** The WAN's servers alone, as OpenWrt ships. */
export const ISP_UPSTREAM: Upstream = { encrypted: [], servers: [], isp: true, strict: false };

/**
 * The card's mode for `upstream`, or `undefined` where it holds more than one mode can: more
 * than one layer, or DoT (the card shows those read-only).
 */
export function modeOf(upstream: Upstream): Mode | undefined {
  const { encrypted, servers, isp, strict } = upstream;
  if (encrypted.length === 0) {
    if (servers.length === 0) return isp ? "isp" : undefined;
    return isp ? undefined : "custom";
  }
  return strict && encrypted.every((r) => r.protocol === "doh") ? "doh" : undefined;
}

/** The DoH resolvers of `upstream`, in order. */
export function dohResolvers(upstream: Upstream): DohResolver[] {
  return upstream.encrypted.flatMap((r) =>
    r.protocol === "doh" ? [{ url: r.url, bootstrap: r.bootstrap }] : [],
  );
}

export type PresetId = "cloudflare" | "quad9" | "google" | "nextdns" | "adguard";
export const PRESET_IDS: readonly PresetId[] = [
  "cloudflare",
  "quad9",
  "google",
  "nextdns",
  "adguard",
];

/** NextDNS's endpoint, followed by the profile id. */
const NEXTDNS = "https://dns.nextdns.io/";

/**
 * The resolvers the UI offers (network.md, Upstream DNS): each with two IPv4 bootstrap servers,
 * as https-dns-proxy's own presets have them (IPv4 works on every WAN; one alone stops the proxy).
 */
export const PRESETS: Record<PresetId, { url: string; bootstrap: readonly string[] }> = {
  cloudflare: { url: "https://cloudflare-dns.com/dns-query", bootstrap: ["1.1.1.1", "1.0.0.1"] },
  quad9: { url: "https://dns.quad9.net/dns-query", bootstrap: ["9.9.9.9", "149.112.112.112"] },
  google: { url: "https://dns.google/dns-query", bootstrap: ["8.8.8.8", "8.8.4.4"] },
  nextdns: { url: NEXTDNS, bootstrap: ["45.90.28.0", "45.90.30.0"] },
  adguard: {
    url: "https://dns.adguard-dns.com/dns-query",
    bootstrap: ["94.140.14.14", "94.140.15.15"],
  },
};

/** A resolver as its row holds it: a preset's (NextDNS with its profile id) or any URL. */
export interface ResolverDraft {
  preset: PresetId | "custom";
  url: string;
  profile: string;
  /** The bootstrap servers as typed, separated by commas or spaces. */
  bootstrap: string;
}

export interface UpstreamDraft {
  mode: Mode;
  servers: string[];
  resolvers: ResolverDraft[];
}

export interface UpstreamErrors {
  /** About the list as a whole (none, too many). */
  list?: string;
  servers: (string | undefined)[];
  resolvers: { url?: string; bootstrap?: string }[];
}

export function isIp(text: string): boolean {
  return ipv4(text) !== undefined || isIpv6(text);
}

export function resolverDraft(resolver: DohResolver): ResolverDraft {
  const bootstrap = resolver.bootstrap.join(", ");
  if (resolver.url.startsWith(NEXTDNS) && resolver.url.length > NEXTDNS.length) {
    return {
      preset: "nextdns",
      url: NEXTDNS,
      profile: resolver.url.slice(NEXTDNS.length),
      bootstrap,
    };
  }
  const preset = PRESET_IDS.find((id) => id !== "nextdns" && PRESETS[id].url === resolver.url);
  return { preset: preset ?? "custom", url: resolver.url, profile: "", bootstrap };
}

export function presetResolver(preset: PresetId | "custom"): ResolverDraft {
  if (preset === "custom") return { preset, url: "", profile: "", bootstrap: "" };
  const { url, bootstrap } = PRESETS[preset];
  return { preset, url, profile: "", bootstrap: bootstrap.join(", ") };
}

export function upstreamDraft(upstream: Upstream): UpstreamDraft {
  const mode = modeOf(upstream) ?? "isp";
  const doh = dohResolvers(upstream);
  return {
    mode,
    servers: mode === "custom" ? [...upstream.servers] : [""],
    resolvers: mode === "doh" ? doh.map(resolverDraft) : [presetResolver("quad9")],
  };
}

/** The URL a row stands for. */
export function resolverUrl(draft: ResolverDraft): string {
  return draft.preset === "nextdns" ? `${NEXTDNS}${draft.profile.trim()}` : draft.url.trim();
}

/** A name for a resolver: its preset's, or its URL's host. */
export function resolverName(url: string): string {
  const draft = resolverDraft({ url, bootstrap: [] });
  if (draft.preset !== "custom") return strings.networks.upstream.presets[draft.preset] ?? url;
  return dohHost(url) ?? url;
}

/** The host of an `https://` URL (brackets dropped), as the model's `doh_host` reads it. */
export function dohHost(url: string): string | undefined {
  if (url.length > 256 || /[\s'"\\,]/.test(url) || !url.startsWith("https://")) return undefined;
  const authority = url.slice(8).split(/[/?#]/)[0] ?? "";
  const match = authority.match(/^(?:\[([0-9a-f:.]+)\]|([^:[\]]+))(?::(\d{1,5}))?$/i);
  if (!match) return undefined;
  const port = match[3];
  if (port !== undefined && (Number(port) === 0 || Number(port) > 65535)) return undefined;
  if (match[1] !== undefined) return isIpv6(match[1]) ? match[1] : undefined;
  const host = match[2] ?? "";
  return ipv4(host) !== undefined || (isDnsName(host) && host.includes(".")) ? host : undefined;
}

/** Splits a typed list on commas and spaces. */
export function splitList(text: string): string[] {
  return text.split(/[\s,]+/).filter((part) => part !== "");
}

/** Each address's error: not an address, or named before. */
function addressErrors(list: readonly string[]): (string | undefined)[] {
  const seen = new Set<string>();
  return list.map((text) => {
    if (!isIp(text)) return e.ip;
    const key = text.toLowerCase();
    if (seen.has(key)) return e.twice;
    seen.add(key);
    return undefined;
  });
}

/** The upstream a draft describes, or what is wrong with it, checked as the server checks it. */
export function upstreamOf(draft: UpstreamDraft): { upstream?: Upstream; errors: UpstreamErrors } {
  const errors: UpstreamErrors = { servers: [], resolvers: [] };
  if (draft.mode === "isp") return { upstream: ISP_UPSTREAM, errors };
  if (draft.mode === "custom") {
    const servers = draft.servers.map((s) => s.trim());
    if (servers.length === 0 || servers.length > MAX) errors.list = e.servers;
    errors.servers = addressErrors(servers);
    const bad = errors.list !== undefined || errors.servers.some(Boolean);
    const upstream: Upstream = { encrypted: [], servers, isp: false, strict: false };
    return bad ? { errors } : { upstream, errors };
  }
  if (draft.resolvers.length === 0 || draft.resolvers.length > MAX) errors.list = e.resolvers;
  const urls = new Set<string>();
  const resolvers = draft.resolvers.map((row): DohResolver => {
    const url = resolverUrl(row);
    const bootstrap = splitList(row.bootstrap);
    const found: { url?: string; bootstrap?: string } = {};
    const host = dohHost(url);
    if (row.preset === "nextdns" && !/^[a-z0-9]{1,32}$/i.test(row.profile.trim())) {
      found.url = e.profile;
    } else if (host === undefined) found.url = e.url;
    else if (urls.has(url)) found.url = e.twice;
    urls.add(url);
    const badIp = bootstrap.find((ip) => !isIp(ip));
    const named = host !== undefined && !isIp(host);
    if (badIp !== undefined) found.bootstrap = e.bootstrapIp(badIp);
    else if (
      bootstrap.length === 1 ||
      bootstrap.length > MAX ||
      (named && bootstrap.length === 0)
    ) {
      found.bootstrap = e.bootstrap;
    } else if (addressErrors(bootstrap).some(Boolean)) found.bootstrap = e.twice;
    errors.resolvers.push(found);
    return { url, bootstrap };
  });
  const bad = errors.list !== undefined || errors.resolvers.some((r) => r.url || r.bootstrap);
  const encrypted = resolvers.map((r) => ({ protocol: "doh" as const, ...r }));
  return bad
    ? { errors }
    : { upstream: { encrypted, servers: [], isp: false, strict: true }, errors };
}

/** The server's issues on `services/upstream/…` as the rows' errors; the rest as text. */
export function upstreamErrorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: UpstreamErrors;
  other: string[];
} {
  const errors: UpstreamErrors = { servers: [], resolvers: [] };
  const other: string[] = [];
  for (const issue of issues) {
    const path = issue.path.replace(/^services\/upstream\//, "");
    const server = path.match(/^servers\/(\d+)$/);
    const resolver = path.match(/^encrypted\/(\d+)\/(url|bootstrap)(?:\/\d+)?$/);
    if (path === "servers" || path === "encrypted") errors.list ??= issue.message;
    else if (server) errors.servers[Number(server[1])] ??= issue.message;
    else if (resolver) {
      const at = Number(resolver[1]);
      const row = errors.resolvers[at] ?? {};
      errors.resolvers[at] = row;
      row[resolver[2] as "url" | "bootstrap"] ??= issue.message;
    } else other.push(issue.message);
  }
  return { errors, other };
}

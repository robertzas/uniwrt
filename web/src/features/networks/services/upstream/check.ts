import type { EncryptedResolver } from "../../../../api/gen/EncryptedResolver";
import type { Upstream } from "../../../../api/gen/Upstream";
import { ipv4 } from "../../../../lib/ipv4";
import { isDnsName } from "../../../../lib/names";
import { strings } from "../../../../lib/strings";
import { isIpv6 } from "../../dns";
import type { DohDraft, DotDraft, UpstreamDraft } from "./draft";
import { MAX, NEXTDNS_DOH, NEXTDNS_DOT, PROFILE } from "./presets";

const e = strings.networks.upstream.errors;

export type ResolverField = "url" | "bootstrap" | "name" | "addresses" | "port" | "spki";
/** A row's errors by field; `row` is about the row as a whole (DoT resolvers kept apart). */
export type ResolverErrors = Partial<Record<ResolverField | "row", string>>;

export interface UpstreamErrors {
  /** No layer in effect. */
  layers?: string;
  /** Encrypted only without an encrypted resolver. */
  strict?: string;
  /** About the lists as a whole (too many). */
  encrypted?: string;
  serverList?: string;
  servers: (string | undefined)[];
  resolvers: ResolverErrors[];
}

export const NO_ERRORS: UpstreamErrors = { servers: [], resolvers: [] };

export function isIp(text: string): boolean {
  return ipv4(text) !== undefined || isIpv6(text);
}

/** A key pin as stubby takes it: 32 bytes in padded base64 (the model's `is_spki_pin`). */
export function isSpkiPin(pin: string): boolean {
  return /^[A-Za-z0-9+/]{42}[AEIMQUYcgkosw048]=$/.test(pin);
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

/** What is wrong with a list of addresses, `min` to [`MAX`] long, or `undefined`. */
function listError(list: readonly string[], min: number, count: string): string | undefined {
  const bad = list.find((ip) => !isIp(ip));
  if (bad !== undefined) return e.notIp(bad);
  if (list.length < min || list.length > MAX) return count;
  return addressErrors(list).some(Boolean) ? e.twice : undefined;
}

function dohOf(row: DohDraft, found: ResolverErrors, urls: Set<string>): EncryptedResolver {
  const url = row.preset === "nextdns" ? `${NEXTDNS_DOH}${row.profile.trim()}` : row.url.trim();
  const bootstrap = splitList(row.bootstrap);
  const host = dohHost(url);
  if (row.preset === "nextdns" && !PROFILE.test(row.profile.trim())) found.url = e.profile;
  else if (host === undefined) found.url = e.url;
  else if (urls.has(url)) found.url = e.twice;
  urls.add(url);
  // A named host needs 2 to 4 bootstrap servers; an address none, or 2 to 4.
  const min = host !== undefined && !isIp(host) ? 2 : bootstrap.length === 0 ? 0 : 2;
  found.bootstrap = listError(bootstrap, min, e.bootstrap);
  return { protocol: "doh", url, bootstrap };
}

function dotOf(row: DotDraft, found: ResolverErrors, names: Set<string>): EncryptedResolver {
  const name = row.preset === "nextdns" ? `${row.profile.trim()}${NEXTDNS_DOT}` : row.name.trim();
  const text = row.port.trim();
  const port = /^\d{1,5}$/.test(text) ? Number(text) : 0;
  if (port < 1 || port > 65535) found.port = e.port;
  if (row.preset === "nextdns" && !PROFILE.test(row.profile.trim())) found.name = e.profile;
  else if (!(isDnsName(name) && name.includes("."))) found.name = e.name;
  else if (names.has(`${name}#${port}`)) found.name = e.twice;
  names.add(`${name}#${port}`);
  const addresses = splitList(row.addresses);
  found.addresses = listError(addresses, 1, e.addresses);
  const spki = splitList(row.spki).map((pin) => pin.replace(/^sha256\//, ""));
  const badPin = spki.find((pin) => !isSpkiPin(pin));
  if (spki.length > MAX) found.spki = e.pins;
  else if (badPin !== undefined) found.spki = e.pin(badPin);
  return { protocol: "dot", name, addresses, port, spki };
}

/** The upstream a draft describes, or what is wrong with it, checked as the server checks it. */
export function upstreamOf(draft: UpstreamDraft): { upstream?: Upstream; errors: UpstreamErrors } {
  const errors: UpstreamErrors = { servers: [], resolvers: [] };
  const servers = draft.servers.map((s) => s.trim());
  if (servers.length > MAX) errors.serverList = e.servers;
  errors.servers = addressErrors(servers);
  if (draft.encrypted.length > MAX) errors.encrypted = e.resolvers;
  const urls = new Set<string>();
  const names = new Set<string>();
  const encrypted = draft.encrypted.map((row) => {
    const found: ResolverErrors = {};
    errors.resolvers.push(found);
    return row.protocol === "doh" ? dohOf(row, found, urls) : dotOf(row, found, names);
  });
  if (draft.strict && encrypted.length === 0) errors.strict = e.strict;
  else if (encrypted.length === 0 && servers.length === 0 && !draft.isp) errors.layers = e.layers;
  const bad =
    errors.layers ??
    errors.strict ??
    errors.encrypted ??
    errors.serverList ??
    (errors.servers.find(Boolean) ||
      errors.resolvers.flatMap((r) => Object.values(r)).find(Boolean));
  if (bad) return { errors };
  return { upstream: { encrypted, servers, isp: draft.isp, strict: draft.strict }, errors };
}

const FIELD = /^encrypted\/(\d+)(?:\/(url|bootstrap|name|addresses|port|spki)(?:\/\d+)?)?$/;

/** The server's issues on `services/upstream…` as the editor's errors; the rest as text. */
export function upstreamErrorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: UpstreamErrors;
  other: string[];
} {
  const errors: UpstreamErrors = { servers: [], resolvers: [] };
  const other: string[] = [];
  for (const issue of issues) {
    if (issue.path === "services/upstream") {
      errors.layers ??= issue.message;
      continue;
    }
    if (!issue.path.startsWith("services/upstream/")) {
      other.push(issue.message);
      continue;
    }
    const path = issue.path.slice("services/upstream/".length);
    const server = path.match(/^servers\/(\d+)$/);
    const resolver = path.match(FIELD);
    if (path === "strict") errors.strict ??= issue.message;
    else if (path === "encrypted") errors.encrypted ??= issue.message;
    else if (path === "servers") errors.serverList ??= issue.message;
    else if (server) errors.servers[Number(server[1])] ??= issue.message;
    else if (resolver) {
      const at = Number(resolver[1]);
      const row = errors.resolvers[at] ?? {};
      errors.resolvers[at] = row;
      row[(resolver[2] as ResolverField | undefined) ?? "row"] ??= issue.message;
    } else other.push(issue.message);
  }
  return { errors, other };
}

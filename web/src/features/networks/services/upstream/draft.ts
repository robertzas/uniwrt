import type { DohResolver } from "../../../../api/gen/DohResolver";
import type { DotResolver } from "../../../../api/gen/DotResolver";
import type { EncryptedResolver } from "../../../../api/gen/EncryptedResolver";
import type { Upstream } from "../../../../api/gen/Upstream";
import { strings } from "../../../../lib/strings";
import { dohHost } from "./check";
import {
  DOH_PRESETS,
  DOT_PORT,
  DOT_PRESETS,
  NEXTDNS_DOH,
  NEXTDNS_DOT,
  PRESET_IDS,
  PROFILE,
} from "./presets";

const t = strings.networks.upstream;

export type PresetId = "cloudflare" | "quad9" | "google" | "nextdns" | "adguard";
export type Protocol = EncryptedResolver["protocol"];

/** A DoH resolver as its row holds it: a preset's (NextDNS with its profile id) or any URL. */
export interface DohDraft {
  protocol: "doh";
  preset: PresetId | "custom";
  url: string;
  profile: string;
  /** The bootstrap servers as typed, separated by commas or spaces. */
  bootstrap: string;
}

/** A DoT resolver as its row holds it: a preset's (NextDNS with its profile id) or any server. */
export interface DotDraft {
  protocol: "dot";
  preset: PresetId | "custom";
  name: string;
  profile: string;
  /** Lists as typed, separated by commas or spaces. */
  addresses: string;
  port: string;
  spki: string;
}

export type ResolverDraft = DohDraft | DotDraft;

/** The layers as the editor holds them (D-146). */
export interface UpstreamDraft {
  encrypted: ResolverDraft[];
  servers: string[];
  isp: boolean;
  strict: boolean;
}

/** The WAN's servers alone, as OpenWrt ships. */
export const ISP_UPSTREAM: Upstream = { encrypted: [], servers: [], isp: true, strict: false };

export function dohDraft(resolver: DohResolver): DohDraft {
  const bootstrap = resolver.bootstrap.join(", ");
  const base = { protocol: "doh" as const, url: resolver.url, profile: "", bootstrap };
  const profile = resolver.url.startsWith(NEXTDNS_DOH)
    ? resolver.url.slice(NEXTDNS_DOH.length)
    : "";
  if (PROFILE.test(profile)) return { ...base, preset: "nextdns", url: NEXTDNS_DOH, profile };
  const preset = PRESET_IDS.find((id) => id !== "nextdns" && DOH_PRESETS[id].url === resolver.url);
  return { ...base, preset: preset ?? "custom" };
}

export function dotDraft(resolver: DotResolver): DotDraft {
  const base = {
    protocol: "dot" as const,
    name: resolver.name,
    profile: "",
    addresses: resolver.addresses.join(", "),
    port: String(resolver.port),
    spki: resolver.spki.join(", "),
  };
  if (resolver.port !== DOT_PORT) return { ...base, preset: "custom" };
  const name = resolver.name.toLowerCase();
  const profile = name.endsWith(NEXTDNS_DOT) ? name.slice(0, -NEXTDNS_DOT.length) : "";
  if (PROFILE.test(profile)) return { ...base, preset: "nextdns", profile };
  const preset = PRESET_IDS.find((id) => {
    const { name: own, aliases = [] } = DOT_PRESETS[id];
    return id !== "nextdns" && (own === name || aliases.includes(name));
  });
  return { ...base, preset: preset ?? "custom" };
}

export function resolverDraft(resolver: EncryptedResolver): ResolverDraft {
  return resolver.protocol === "doh" ? dohDraft(resolver) : dotDraft(resolver);
}

/** A new row: a preset's values, or empty fields for any resolver. */
export function presetResolver(protocol: Protocol, preset: PresetId | "custom"): ResolverDraft {
  if (protocol === "doh") {
    if (preset === "custom") return { protocol, preset, url: "", profile: "", bootstrap: "" };
    const { url, bootstrap } = DOH_PRESETS[preset];
    return { protocol, preset, url, profile: "", bootstrap: bootstrap.join(", ") };
  }
  const port = String(DOT_PORT);
  if (preset === "custom") {
    return { protocol, preset, name: "", profile: "", addresses: "", port, spki: "" };
  }
  const { name, addresses } = DOT_PRESETS[preset];
  return { protocol, preset, name, profile: "", addresses: addresses.join(", "), port, spki: "" };
}

export function upstreamDraft(upstream: Upstream): UpstreamDraft {
  return {
    encrypted: keepDotTogether(upstream.encrypted.map(resolverDraft)),
    servers: [...upstream.servers],
    isp: upstream.isp,
    strict: upstream.strict,
  };
}

/** A resolver's name: its preset's, or its URL's host or certificate name. */
export function resolverName(resolver: EncryptedResolver): string {
  const draft = resolverDraft(resolver);
  if (draft.preset !== "custom") return t.presets[draft.preset] ?? draft.preset;
  if (resolver.protocol === "dot") return resolver.name;
  return dohHost(resolver.url) ?? resolver.url;
}

/** A row's name, with its protocol: `Quad9 (DoT)`. */
export function rowName(row: ResolverDraft, index: number): string {
  const name = row.preset === "custom" ? t.resolver(index + 1) : t.presets[row.preset];
  return t.rowName(name ?? row.preset, t.short[row.protocol] ?? row.protocol);
}

/** What a revision's message says the upstream is: `Quad9 (DoT) → 9.9.9.9 → the ISP's`. */
export function describeUpstream(upstream: Upstream): string {
  const encrypted = upstream.encrypted
    .map((r) => t.rowName(resolverName(r), t.short[r.protocol] ?? r.protocol))
    .join(", ");
  if (upstream.strict && encrypted !== "") return t.describe.strict(encrypted);
  const layers = [encrypted, upstream.servers.join(", "), upstream.isp ? t.describe.isp : ""];
  return layers.filter((layer) => layer !== "").join(" → ");
}

/**
 * The list in units of order: each DoH resolver alone, the DoT resolvers as one, in the first
 * one's place (stubby asks them in turn; the model refuses them apart).
 */
function units(list: readonly ResolverDraft[]): ResolverDraft[][] {
  const out: ResolverDraft[][] = [];
  let dot: ResolverDraft[] | undefined;
  for (const row of list) {
    if (row.protocol === "doh") out.push([row]);
    else if (dot) dot.push(row);
    else {
      dot = [row];
      out.push(dot);
    }
  }
  return out;
}

/** The list with every DoT resolver moved next to the first one. */
export function keepDotTogether(list: readonly ResolverDraft[]): ResolverDraft[] {
  return units(list).flat();
}

/** The list with `row` added: last, or a DoT resolver after the other DoT ones. */
export function withResolver(list: readonly ResolverDraft[], row: ResolverDraft): ResolverDraft[] {
  if (row.protocol === "doh") return [...list, row];
  const last = list.findLastIndex((r) => r.protocol === "dot");
  return last < 0 ? [...list, row] : [...list.slice(0, last + 1), row, ...list.slice(last + 1)];
}

/**
 * The list with the row at `index` moved one place up (-1) or down (1). DoT resolvers swap among
 * themselves; past a DoH resolver, they move together.
 */
export function moveResolver(
  list: readonly ResolverDraft[],
  index: number,
  by: -1 | 1,
): ResolverDraft[] {
  const next = [...list];
  const a = next[index];
  const b = next[index + by];
  if (!a || !b) return next;
  if (a.protocol === "dot" && b.protocol === "dot") {
    next[index] = b;
    next[index + by] = a;
    return next;
  }
  const groups = units(next);
  const at = groups.findIndex((group) => group.includes(a));
  const other = groups[at + by];
  const own = groups[at];
  if (!own || !other) return next;
  groups[at] = other;
  groups[at + by] = own;
  return groups.flat();
}

import type { PresetId } from "./draft";

/** The most resolvers, servers, bootstrap servers, DoT addresses and key pins (`MAX_SERVERS`). */
export const MAX = 4;

/** The port DNS over TLS listens on unless a resolver says otherwise (RFC 7858, `DOT_PORT`). */
export const DOT_PORT = 853;

export const PRESET_IDS: readonly PresetId[] = [
  "cloudflare",
  "quad9",
  "google",
  "nextdns",
  "adguard",
];

/** NextDNS's DoH endpoint, followed by the profile id. */
export const NEXTDNS_DOH = "https://dns.nextdns.io/";
/** NextDNS's DoT name, after the profile id. */
export const NEXTDNS_DOT = ".dns.nextdns.io";
/** A NextDNS profile id. */
export const PROFILE = /^[a-z0-9]{1,32}$/i;

/**
 * The DoH resolvers the UI offers (network.md, Upstream DNS): each with two IPv4 bootstrap
 * servers, as https-dns-proxy's own presets have them (IPv4 works on every WAN; one alone stops
 * the proxy).
 */
export const DOH_PRESETS: Record<PresetId, { url: string; bootstrap: readonly string[] }> = {
  cloudflare: { url: "https://cloudflare-dns.com/dns-query", bootstrap: ["1.1.1.1", "1.0.0.1"] },
  quad9: { url: "https://dns.quad9.net/dns-query", bootstrap: ["9.9.9.9", "149.112.112.112"] },
  google: { url: "https://dns.google/dns-query", bootstrap: ["8.8.8.8", "8.8.4.4"] },
  nextdns: { url: NEXTDNS_DOH, bootstrap: ["45.90.28.0", "45.90.30.0"] },
  adguard: {
    url: "https://dns.adguard-dns.com/dns-query",
    bootstrap: ["94.140.14.14", "94.140.15.15"],
  },
};

/**
 * The DoT resolvers the UI offers: the name on each service's certificate (NextDNS's after the
 * profile id) and two IPv4 addresses, on port 853. No key pins: a service that rotates its key
 * would stop resolving (D-146); an admin adds pins on purpose.
 */
export const DOT_PRESETS: Record<
  PresetId,
  { name: string; aliases?: readonly string[]; addresses: readonly string[] }
> = {
  cloudflare: {
    name: "one.one.one.one",
    aliases: ["cloudflare-dns.com"],
    addresses: ["1.1.1.1", "1.0.0.1"],
  },
  quad9: { name: "dns.quad9.net", addresses: ["9.9.9.9", "149.112.112.112"] },
  google: { name: "dns.google", addresses: ["8.8.8.8", "8.8.4.4"] },
  nextdns: { name: NEXTDNS_DOT, addresses: ["45.90.28.0", "45.90.30.0"] },
  adguard: { name: "dns.adguard-dns.com", addresses: ["94.140.14.14", "94.140.15.15"] },
};

import type { BlocklistsView } from "../api/gen/BlocklistsView";
import type { CatalogList } from "../api/gen/CatalogList";
import type { DnsQuery } from "../api/gen/DnsQuery";
import type { DnsStats } from "../api/gen/DnsStats";

/** Part of the controller's catalog (`site::blocking::CATALOG`). */
export const CATALOG: CatalogList[] = [
  {
    id: "hagezi_normal",
    name: "HaGeZi Multi Normal",
    category: "ads",
    url: "https://example.com/hagezi-normal.txt",
    domains: 300_000,
  },
  {
    id: "oisd_big",
    name: "OISD Big",
    category: "ads",
    url: "https://example.com/oisd-big.txt",
    domains: 250_000,
  },
  {
    id: "urlhaus",
    name: "URLhaus",
    category: "malware",
    url: "https://example.com/urlhaus.txt",
    domains: 3_000,
  },
];

/** DNS filtering's state as `GET v1/services/blocklists` serves it: off, nothing downloaded. */
export function blocklistsView(extra: Partial<BlocklistsView> = {}): BlocklistsView {
  return {
    catalog: CATALOG,
    gateway: 1,
    enabled: false,
    lists: [],
    compiled: null,
    sync: { state: "off", sha256: null, domains: null, pushed_at: null, error: null },
    refreshing: false,
    refreshed_at: null,
    ...extra,
  };
}

/** Statistics as `GET v1/services/dns/stats` serves them: none, unless `extra` adds some. */
export function dnsStats(extra: Partial<DnsStats> = {}): DnsStats {
  const now = Date.now();
  return {
    range: "24h",
    since: now - 24 * 3_600_000,
    queries: 0,
    blocked: 0,
    hours: [],
    clients: [],
    domains_since: now - 24 * 3_600_000,
    top_domains: [],
    top_blocked: [],
    ...extra,
  };
}

/** A stored query from the default client. */
export function dnsQuery(extra: Partial<DnsQuery> = {}): DnsQuery {
  return {
    at: Date.now(),
    client: "192.168.1.20",
    mac: "aa:00:00:00:00:01",
    qtype: "A",
    domain: "example.com",
    outcome: "forwarded",
    ...extra,
  };
}

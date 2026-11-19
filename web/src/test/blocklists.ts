import type { BlocklistsView } from "../api/gen/BlocklistsView";
import type { CatalogList } from "../api/gen/CatalogList";

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

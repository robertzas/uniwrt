//! DNS filtering on the gateway (features/network.md, DHCP & local DNS: Blocklists; D-132,
//! D-133): the lists the controller compiles into one file of dnsmasq `server=/domain/` lines,
//! the domains always allowed or blocked, and the clients whose DNS goes to an unfiltered
//! resolver. Schema v13 adds it, off.

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

/// DNS filtering: off until turned on; the catalog's lists [`DEFAULT_LISTS`] preselected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Blocking {
    pub enabled: bool,
    /// Lists of the [`CATALOG`], by id.
    pub lists: Vec<String>,
    /// Lists from any URL, in hosts, plain domain, adblock (`||domain^`) or dnsmasq format.
    pub custom: Vec<CustomList>,
    /// Domains never blocked, with their subdomains, whatever the lists say.
    pub allow: Vec<String>,
    /// Domains always blocked, with their subdomains.
    pub deny: Vec<String>,
    /// Clients whose DNS (port 53, IPv4 and IPv6) the gateway's firewall redirects to an
    /// unfiltered dnsmasq instance ([`UNFILTERED_PORT`]).
    #[cfg_attr(feature = "ts", ts(as = "Vec<String>"))]
    pub bypass: Vec<MacAddr>,
}

impl Default for Blocking {
    fn default() -> Self {
        Self {
            enabled: false,
            lists: DEFAULT_LISTS.iter().map(|&id| id.to_owned()).collect(),
            custom: Vec::new(),
            allow: Vec::new(),
            deny: Vec::new(),
            bypass: Vec::new(),
        }
    }
}

impl Blocking {
    /// Whether the gateway renders the unfiltered instance and the redirects to it.
    pub fn bypasses(&self) -> bool {
        self.enabled && !self.bypass.is_empty()
    }
}

/// A list from a URL of the user's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CustomList {
    /// What the UI calls it.
    pub name: String,
    /// An `http://` or `https://` URL.
    pub url: String,
}

/// What a catalog list blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ListCategory {
    /// Ads and trackers.
    Ads,
    /// Malware, phishing and scams.
    Malware,
}

/// A list the controller offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CatalogList {
    pub id: &'static str,
    pub name: &'static str,
    pub category: ListCategory,
    pub url: &'static str,
    /// About how many domains it holds (2026-11), for the UI's size estimate.
    pub domains: u32,
}

/// The lists the controller offers, smallest first within each category. Each is in a format
/// the compiler reads (domains, hosts or dnsmasq lines).
pub const CATALOG: &[CatalogList] = &[
    list(
        "hagezi_light",
        "HaGeZi Light",
        ListCategory::Ads,
        "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/light-onlydomains.txt",
        93_000,
    ),
    list(
        "hagezi_normal",
        "HaGeZi Normal",
        ListCategory::Ads,
        "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/multi-onlydomains.txt",
        188_000,
    ),
    list(
        "hagezi_pro",
        "HaGeZi Pro",
        ListCategory::Ads,
        "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/pro-onlydomains.txt",
        253_000,
    ),
    list(
        "oisd_small",
        "OISD Small",
        ListCategory::Ads,
        "https://small.oisd.nl/dnsmasq2",
        63_000,
    ),
    list(
        "oisd_big",
        "OISD Big",
        ListCategory::Ads,
        "https://big.oisd.nl/dnsmasq2",
        230_000,
    ),
    list(
        "stevenblack",
        "StevenBlack Unified",
        ListCategory::Ads,
        "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
        79_000,
    ),
    // abuse.ch's own hostfile is gone (410 in 2026-11); malware-filter mirrors its domains.
    list(
        "urlhaus",
        "URLhaus",
        ListCategory::Malware,
        "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-hosts.txt",
        2_000,
    ),
    list(
        "phishing",
        "Phishing Filter",
        ListCategory::Malware,
        "https://malware-filter.gitlab.io/malware-filter/phishing-filter-hosts.txt",
        17_000,
    ),
    list(
        "hagezi_tif_mini",
        "HaGeZi Threat Intelligence Mini",
        ListCategory::Malware,
        "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/tif.mini-onlydomains.txt",
        189_000,
    ),
];

const fn list(
    id: &'static str,
    name: &'static str,
    category: ListCategory,
    url: &'static str,
    domains: u32,
) -> CatalogList {
    CatalogList {
        id,
        name,
        category,
        url,
        domains,
    }
}

/// The lists a site starts with, used once blocking is turned on (the owner's choice, D-133).
pub const DEFAULT_LISTS: &[&str] = &["hagezi_normal", "urlhaus"];

/// The catalog's list `id`.
pub fn catalog_list(id: &str) -> Option<&'static CatalogList> {
    CATALOG.iter().find(|list| list.id == id)
}

/// The directory the gateway's main dnsmasq reads `*.conf` files from while blocking is on
/// (`confdir '<dir>,*.conf'`). The controller pushes [`BLOCKLIST_FILE`] there, then
/// [`BLOCKLIST_CONF`], which loads it: a `servers-file` that dnsmasq reads again on SIGHUP.
/// Until the first push the directory holds nothing and dnsmasq starts unfiltered (a missing
/// `servers-file` would keep it from starting).
pub const CONF_DIR: &str = "/tmp/dnsmasq.uniwrt.d";
/// The compiled list in [`CONF_DIR`]: a header line, then `server=/<domain>/` (blocked) and
/// `server=/<domain>/#` (allowed under a blocked parent) lines.
pub const BLOCKLIST_FILE: &str = "/tmp/dnsmasq.uniwrt.d/blocklist.servers";
/// The conf file in [`CONF_DIR`] that names [`BLOCKLIST_FILE`] as dnsmasq's `servers-file`.
pub const BLOCKLIST_CONF: &str = "/tmp/dnsmasq.uniwrt.d/blocklist.conf";
/// The port of the unfiltered dnsmasq instance the bypassed clients' DNS is redirected to.
pub const UNFILTERED_PORT: u16 = 5354;
/// The dnsmasq sections' names while a bypass is rendered: the main instance must be named
/// for the pools to name it (`option instance`), or the unfiltered one would serve them too.
pub const MAIN_INSTANCE: &str = "main";
pub const UNFILTERED_INSTANCE: &str = "unfiltered";

#[cfg(test)]
mod tests {
    use super::{CATALOG, DEFAULT_LISTS, catalog_list};

    #[test]
    fn the_catalog_holds_its_defaults_once_each_with_a_url() {
        for id in DEFAULT_LISTS {
            assert!(catalog_list(id).is_some(), "{id}");
        }
        let mut ids: Vec<_> = CATALOG.iter().map(|l| l.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for list in CATALOG {
            assert!(
                list.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{}",
                list.id
            );
            assert!(list.url.starts_with("https://"), "{}", list.url);
        }
    }
}

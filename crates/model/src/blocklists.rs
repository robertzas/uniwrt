//! DNS blocklists as the controller holds them (features/network.md, Blocklists; D-133): each
//! list's download state, the compiled file's summary, how many domains a gateway can hold, and
//! the header line that names the compiled file on the gateway, and the state the API shows
//! ([`BlocklistState`]). The settings are
//! [`site::blocking`](crate::site::blocking).

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, Timestamp};

/// One list's download state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ListStatus {
    /// The catalog's id; `None` for a custom list.
    pub id: Option<String>,
    pub name: String,
    pub url: String,
    /// When the controller last asked for it and got it, changed or not.
    pub checked_at: Option<Timestamp>,
    /// When its content last changed.
    pub fetched_at: Option<Timestamp>,
    /// The domains read from the copy kept; `None` until one is.
    pub domains: Option<u32>,
    /// Why the last attempt failed; the last good copy is still used.
    pub error: Option<String>,
}

/// The compiled file: what the gateway gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CompiledStatus {
    /// The SHA-256 of its lines (hex), as in its [`header`].
    pub sha256: String,
    /// The domains blocked (each with its subdomains).
    pub domains: u32,
    /// The domains left out for the gateway's memory ([`capacity`]).
    pub cut: u32,
    /// The most domains the gateway holds.
    pub capacity: u32,
    pub at: Timestamp,
}

/// DNS filtering as the controller keeps it: the lists, the compiled file and the gateway's
/// copy (`GET v1/services/blocklists`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BlocklistState {
    /// The gateway, once the controller knows one.
    pub gateway: Option<NodeId>,
    /// Whether blocking is on in the revision applied to the gateway (the one pushed).
    pub enabled: bool,
    /// The lists of the saved and the applied revisions (where blocking is on), most wanted
    /// first.
    pub lists: Vec<ListStatus>,
    /// The file compiled for the gateway, once there is one.
    pub compiled: Option<CompiledStatus>,
    pub sync: GatewaySync,
    /// Whether the lists are being downloaded now.
    pub refreshing: bool,
    /// When the least recently checked list was asked for; `None` while one never was.
    pub refreshed_at: Option<Timestamp>,
}

/// Where the gateway's copy of the compiled file stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GatewaySync {
    pub state: SyncState,
    /// The SHA-256 (hex) of the file the gateway reports holding, and its domains.
    pub sha256: Option<String>,
    pub domains: Option<u32>,
    /// When the controller last pushed the file.
    pub pushed_at: Option<Timestamp>,
    /// Why the last push failed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// No adopted node is the gateway.
    NoGateway,
    /// The revision applied to the gateway doesn't manage its DHCP and DNS: the controller
    /// leaves its files alone.
    Unmanaged,
    /// Blocking is off in the revision applied to the gateway.
    Off,
    Offline,
    /// A job holds the gateway.
    Busy,
    /// The lists are being downloaded or compiled.
    Waiting,
    Pushing,
    /// The gateway holds the compiled file.
    Synced,
    /// The last push failed; it is tried again.
    Failed,
}

/// About what dnsmasq holds per blocked domain (a `server=/domain/` line), in bytes.
pub const BYTES_PER_DOMAIN: u64 = 150;
/// The share of the gateway's memory the list may take: one eighth.
pub const MEMORY_SHARE: u64 = 8;

/// The most domains a gateway with `mem_total_bytes` of memory holds: an eighth of it at
/// [`BYTES_PER_DOMAIN`] each (about 110,000 on 128 MB, 900,000 on 1 GB).
pub fn capacity(mem_total_bytes: u64) -> u32 {
    u32::try_from(mem_total_bytes / MEMORY_SHARE / BYTES_PER_DOMAIN).unwrap_or(u32::MAX)
}

/// The compiled file's first line: `# uniwrt <sha256> <domains>`, a comment to dnsmasq.
pub fn header(sha256: &str, domains: u32) -> String {
    format!("# uniwrt {sha256} {domains}")
}

/// The SHA-256 and domain count of a [`header`] line; `None` for any other line.
pub fn parse_header(line: &str) -> Option<(String, u32)> {
    let mut words = line.trim().strip_prefix("# uniwrt ")?.split(' ');
    let sha256 = words.next()?;
    let domains = words.next()?.parse().ok()?;
    let hex = sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit());
    (hex && words.next().is_none()).then(|| (sha256.to_ascii_lowercase(), domains))
}

#[cfg(test)]
mod tests {
    use super::{capacity, header, parse_header};

    #[test]
    fn headers_read_back_and_odd_lines_are_none() {
        let sha = "ab".repeat(32);
        assert_eq!(parse_header(&header(&sha, 7)), Some((sha.clone(), 7)));
        assert_eq!(
            parse_header(&format!("{}\n", header(&sha, 7))),
            Some((sha, 7))
        );
        for line in [
            "",
            "server=/a.com/",
            "# uniwrt abc 7",
            "# uniwrt",
            "# other",
        ] {
            assert_eq!(parse_header(line), None, "{line}");
        }
        assert_eq!(
            parse_header(&format!("# uniwrt {} 7 x", "0".repeat(64))),
            None
        );
    }

    #[test]
    fn capacity_is_an_eighth_of_memory() {
        assert_eq!(capacity(128 << 20), 111_848);
        assert_eq!(capacity(0), 0);
    }
}

//! DNS blocklists as the controller holds them (features/network.md, Blocklists; D-133): each
//! list's download state, the compiled file's summary, how many domains a gateway can hold, and
//! the header line that names the compiled file on the gateway. The settings are
//! [`site::blocking`](crate::site::blocking).

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

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

//! A packet capture (features/observability.md, Diagnostics toolkit; D-141, D-142): tcpdump on
//! one node, streamed to the browser as a `.pcap` download with a time and a size cap.
//! [`CaptureQuery`] is what an admin asks for (the download's query string), checked by
//! [`CaptureQuery::validate`] before anything reaches a node: the filter reaches tcpdump as one
//! argument after `--`, never as shell text, and must be a whole parenthesized expression, since
//! the node appends the controller's own SSH session to it as `and not (…)`.

use serde::{Deserialize, Serialize};

use super::is_ifname;

/// The device that captures on every network device (with Linux cooked headers).
pub const ANY: &str = "any";
/// How long a capture runs when not told, in seconds.
pub const DEFAULT_SECONDS: u32 = 60;
/// The longest capture, in seconds.
pub const MAX_SECONDS: u32 = 3600;
/// The size a capture stops at when not told, in MiB.
pub const DEFAULT_MIB: u32 = 16;
/// The largest capture, in MiB.
pub const MAX_MIB: u32 = 256;
/// The shortest snapshot length: an Ethernet, IPv6 and TCP header with options.
pub const MIN_SNAPLEN: u32 = 96;
/// The longest snapshot length, tcpdump's default.
pub const MAX_SNAPLEN: u32 = 262_144;
/// The longest filter.
pub const MAX_FILTER_LEN: usize = 1024;

/// What `GET v1/nodes/{id}/diag/capture` asks: where to capture, what, and for how long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, deny_unknown_fields)]
pub struct CaptureQuery {
    /// The network device, such as `br-lan`; [`ANY`] (every device) by default.
    pub interface: String,
    /// A BPF filter (pcap-filter(7)), such as `port 53`; every packet when absent or blank.
    pub filter: Option<String>,
    /// When the capture stops, 1 to [`MAX_SECONDS`].
    pub seconds: u32,
    /// The size it stops at, 1 to [`MAX_MIB`] MiB: before the first packet that would pass it.
    pub max_mib: u32,
    /// The bytes kept of each packet, [`MIN_SNAPLEN`] to [`MAX_SNAPLEN`]; all of it when absent.
    pub snaplen: Option<u32>,
}

impl Default for CaptureQuery {
    fn default() -> Self {
        Self {
            interface: ANY.to_owned(),
            filter: None,
            seconds: DEFAULT_SECONDS,
            max_mib: DEFAULT_MIB,
            snaplen: None,
        }
    }
}

impl CaptureQuery {
    /// The filter, trimmed; `None` when absent or blank.
    pub fn filter(&self) -> Option<&str> {
        self.filter
            .as_deref()
            .map(str::trim)
            .filter(|filter| !filter.is_empty())
    }

    /// The size cap in bytes.
    pub fn max_bytes(&self) -> u64 {
        u64::from(self.max_mib) << 20
    }

    /// Why the capture can't run as asked, worded for people.
    pub fn validate(&self) -> Result<(), String> {
        if self.interface != ANY && !is_ifname(&self.interface) {
            return Err(format!(
                "{:?} is not a network device's name (or \"any\")",
                self.interface
            ));
        }
        if let Some(filter) = self.filter() {
            check_filter(filter)?;
        }
        if !(1..=MAX_SECONDS).contains(&self.seconds) {
            return Err(format!("A capture runs for 1 to {MAX_SECONDS} seconds"));
        }
        if !(1..=MAX_MIB).contains(&self.max_mib) {
            return Err(format!("A capture stops at 1 to {MAX_MIB} MiB"));
        }
        match self.snaplen {
            Some(snaplen) if !(MIN_SNAPLEN..=MAX_SNAPLEN).contains(&snaplen) => Err(format!(
                "A capture keeps {MIN_SNAPLEN} to {MAX_SNAPLEN} bytes of each packet"
            )),
            _ => Ok(()),
        }
    }
}

/// A filter of printable ASCII, not starting with `-`, whose parentheses pair up in order.
fn check_filter(filter: &str) -> Result<(), String> {
    if filter.len() > MAX_FILTER_LEN {
        return Err(format!(
            "A filter is at most {MAX_FILTER_LEN} characters long"
        ));
    }
    if !filter.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
        return Err("A filter is printable ASCII on one line".to_owned());
    }
    if filter.starts_with('-') {
        return Err("A filter can't start with \"-\"".to_owned());
    }
    let mut depth: u32 = 0;
    for b in filter.bytes() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "The filter closes a parenthesis it never opened".to_owned())?;
            }
            _ => {}
        }
    }
    if depth > 0 {
        return Err("The filter leaves a parenthesis open".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ANY, CaptureQuery};

    fn query(filter: &str) -> CaptureQuery {
        CaptureQuery {
            filter: Some(filter.to_owned()),
            ..CaptureQuery::default()
        }
    }

    #[test]
    fn filters_are_whole_expressions() {
        for filter in [
            "port 53",
            "icmp or (udp and port 53)",
            "ether host 02:00:00:00:00:01 and not arp",
            "tcp[tcpflags] & tcp-syn != 0",
            "  ",
        ] {
            assert_eq!(query(filter).validate(), Ok(()), "{filter}");
        }
        assert_eq!(query("  port 53 ").filter(), Some("port 53"));
        assert_eq!(query(" ").filter(), None);
        for filter in [
            "-w /tmp/x",
            "tcp) or (udp",
            "(tcp",
            "port 53\nport 54",
            "host ä",
        ] {
            assert!(query(filter).validate().is_err(), "{filter}");
        }
        assert!(query(&"a".repeat(1025)).validate().is_err());
    }

    #[test]
    fn devices_and_caps_are_bounded() {
        let ok = CaptureQuery::default();
        assert_eq!((ok.interface.as_str(), ok.max_bytes()), (ANY, 16 << 20));
        assert_eq!(ok.validate(), Ok(()));
        for bad in [
            CaptureQuery {
                interface: "-i".to_owned(),
                ..ok.clone()
            },
            CaptureQuery {
                interface: "br lan".to_owned(),
                ..ok.clone()
            },
            CaptureQuery {
                seconds: 0,
                ..ok.clone()
            },
            CaptureQuery {
                seconds: 3601,
                ..ok.clone()
            },
            CaptureQuery {
                max_mib: 257,
                ..ok.clone()
            },
            CaptureQuery {
                snaplen: Some(64),
                ..ok.clone()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        let good = CaptureQuery {
            interface: "br-lan".to_owned(),
            snaplen: Some(128),
            ..ok
        };
        assert_eq!(good.validate(), Ok(()));
    }
}

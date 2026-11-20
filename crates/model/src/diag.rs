//! Diagnostics runs (features/observability.md, Diagnostics toolkit; D-141): a tool run on one
//! node, its output kept by the controller as it comes and streamed to the browser.
//!
//! [`DiagTool`] is what an admin asks for, checked by [`DiagTool::validate`] before anything
//! reaches a node; [`DiagRun`] is a run as the controller holds it.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, Timestamp};

/// The longest host name or address a tool takes.
pub const MAX_TARGET_LEN: usize = 253;
/// The longest URL the HTTP check takes.
pub const MAX_URL_LEN: usize = 2048;
/// The most echo requests a ping sends.
pub const MAX_PING_COUNT: u32 = 100;
/// The echo requests a ping sends when not told.
pub const DEFAULT_PING_COUNT: u32 = 10;

const fn default_ping_count() -> u32 {
    DEFAULT_PING_COUNT
}

/// A tool and its arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiagTool {
    /// ICMP echo requests, one a second (BusyBox `ping`).
    Ping {
        /// A host name or address.
        target: String,
        /// Which address family a name resolves to; the resolver's choice when absent.
        #[serde(default)]
        family: Option<IpFamily>,
        /// At most [`MAX_PING_COUNT`]; [`DEFAULT_PING_COUNT`] when absent.
        #[serde(default = "default_ping_count")]
        count: u32,
    },
    /// The route to a host, hop by hop, with numeric addresses (BusyBox `traceroute -n`).
    Traceroute {
        target: String,
        #[serde(default)]
        family: Option<IpFamily>,
    },
    /// A DNS lookup (BusyBox `nslookup`), through the node's resolver or the `server` named.
    Dns {
        name: String,
        /// Both A and AAAA when absent, as `nslookup` does.
        #[serde(default)]
        record: Option<DnsRecordType>,
        #[serde(default)]
        server: Option<String>,
    },
    /// Fetches a URL and discards the body (`uclient-fetch -O /dev/null`): whether it answers,
    /// and how long that took.
    Http { url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum IpFamily {
    Ipv4,
    Ipv6,
}

/// The record types a DNS lookup asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Ptr,
    Soa,
    Srv,
    Txt,
}

impl DnsRecordType {
    /// The name `nslookup -type=` takes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::Cname => "CNAME",
            Self::Mx => "MX",
            Self::Ns => "NS",
            Self::Ptr => "PTR",
            Self::Soa => "SOA",
            Self::Srv => "SRV",
            Self::Txt => "TXT",
        }
    }
}

impl DiagTool {
    /// What the tool is pointed at, as the audit names it.
    pub fn target(&self) -> &str {
        match self {
            Self::Ping { target, .. } | Self::Traceroute { target, .. } => target,
            Self::Dns { name, .. } => name,
            Self::Http { url } => url,
        }
    }

    /// The tool's name, as the API spells it.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Ping { .. } => "ping",
            Self::Traceroute { .. } => "traceroute",
            Self::Dns { .. } => "dns",
            Self::Http { .. } => "http",
        }
    }

    /// Why the tool can't run as asked, worded for people.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Ping { target, count, .. } => {
                host(target)?;
                if !(1..=MAX_PING_COUNT).contains(count) {
                    return Err(format!("A ping sends 1 to {MAX_PING_COUNT} requests"));
                }
                Ok(())
            }
            Self::Traceroute { target, .. } => host(target),
            Self::Dns { name, server, .. } => {
                if !is_lookup_name(name) && name.parse::<IpAddr>().is_err() {
                    return Err(format!("{name:?} is not a DNS name or an address"));
                }
                match server {
                    Some(server)
                        if server.parse::<IpAddr>().is_err() && !is_lookup_name(server) =>
                    {
                        Err(format!("{server:?} is not a DNS server's name or address"))
                    }
                    _ => Ok(()),
                }
            }
            Self::Http { url } => http_url(url),
        }
    }
}

/// A host name or an address.
fn host(target: &str) -> Result<(), String> {
    if target.parse::<IpAddr>().is_ok() || is_lookup_name(target) {
        Ok(())
    } else {
        Err(format!("{target:?} is not a host name or an address"))
    }
}

/// A name a resolver may be asked about: dot-separated labels of letters, digits, `-` and `_`
/// (service names such as `_sip._tcp`), none starting with `-`, with an optional final dot.
fn is_lookup_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= MAX_TARGET_LEN
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

/// An `http://` or `https://` URL with a host, of printable ASCII.
fn http_url(url: &str) -> Result<(), String> {
    let bad = || format!("{url:?} is not an http:// or https:// URL");
    if url.len() > MAX_URL_LEN {
        return Err(format!("A URL is at most {MAX_URL_LEN} characters long"));
    }
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(bad)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(bad());
    }
    Ok(())
}

/// Where a run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    /// The tool exited with 0.
    Succeeded,
    /// The tool exited otherwise, or never ran ([`DiagRun::error`] says why).
    Failed,
    /// Stopped by an admin, at its time limit, or once nobody watched it.
    Stopped,
}

/// One line of a tool's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagLine {
    /// When the controller received it.
    pub at: Timestamp,
    /// Written to stderr.
    #[serde(default)]
    pub err: bool,
    pub text: String,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagEnd {
    pub state: RunState,
    pub ended_at: Timestamp,
    pub exit_code: Option<u32>,
    pub error: Option<String>,
}

/// A run as the controller holds it: in memory, the last 256 KiB of its output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DiagRun {
    pub id: u32,
    pub node: NodeId,
    pub tool: DiagTool,
    pub state: RunState,
    /// Who started it.
    pub by: Option<String>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    /// The tool's exit code, once it exited.
    pub exit_code: Option<u32>,
    /// Why it failed or was stopped, worded for people.
    pub error: Option<String>,
    /// The lines kept, oldest first (empty in lists).
    pub lines: Vec<DiagLine>,
    /// Lines dropped from the start to keep the output within its budget.
    pub dropped: u32,
}

#[cfg(test)]
mod tests;

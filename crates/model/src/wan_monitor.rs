//! The WAN quality monitor (features/observability.md, Troubleshooting assistants; D-144): what
//! the gateway probes every [`INTERVAL`] ([`WanMonitorSettings`], kept in the controller's
//! settings), what a round of probes found ([`ProbeRound`]), and what that says of the
//! internet connection ([`assess`]).
//!
//! An ICMP target gets [`PINGS`] pings a second apart, so its round has bands (the fastest,
//! the median and the slowest answer, smokeping-style); a DNS or HTTP target is one timed
//! lookup or fetch. A DNS target through the WAN's `upstreams` is one series per server the
//! WAN learned (`<id>@<address>`). The internet is down when every internet target (an ICMP
//! target other than the first hop, or an HTTP one) lost every probe; where the fault lies
//! follows from the rest ([`WanFault`]).

use std::{net::IpAddr, time::Duration};

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// How often the gateway probes.
pub const INTERVAL: Duration = Duration::from_secs(30);
/// Pings per ICMP target and round, one a second.
pub const PINGS: u8 = 10;
pub const TARGETS_MAX: usize = 12;
const ID_MAX: usize = 24;
const NAME_MAX: usize = 48;
const URL_MAX: usize = 255;

/// What the gateway probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct WanMonitorSettings {
    pub enabled: bool,
    pub targets: Vec<ProbeTarget>,
}

impl WanMonitorSettings {
    /// Its key among the controller's settings.
    pub const KEY: &'static str = "wan_monitor";

    /// Why the settings can't be saved, if they can't.
    pub fn validate(&self) -> Result<(), String> {
        if self.targets.len() > TARGETS_MAX {
            return Err(format!("At most {TARGETS_MAX} targets"));
        }
        for (i, target) in self.targets.iter().enumerate() {
            target.validate()?;
            if self.targets[..i].iter().any(|other| other.id == target.id) {
                return Err(format!("Two targets are named {}", target.id));
            }
        }
        Ok(())
    }

    /// The target a series key (`<id>` or `<id>@<address>`) belongs to.
    pub fn target(&self, key: &str) -> Option<&ProbeTarget> {
        let id = key.split_once('@').map_or(key, |(id, _)| id);
        self.targets.iter().find(|target| target.id == id)
    }
}

impl Default for WanMonitorSettings {
    fn default() -> Self {
        let icmp = |id: &str, name: &str, host: Option<&str>| ProbeTarget {
            id: id.to_owned(),
            name: name.to_owned(),
            probe: Probe::Icmp {
                host: host.map(str::to_owned),
            },
        };
        let dns = |id: &str, name: &str, via: DnsVia| ProbeTarget {
            id: id.to_owned(),
            name: name.to_owned(),
            probe: Probe::Dns {
                via,
                name: "openwrt.org".to_owned(),
            },
        };
        Self {
            enabled: true,
            targets: vec![
                icmp("first-hop", "ISP gateway", None),
                icmp("cloudflare", "Cloudflare", Some("1.1.1.1")),
                icmp("google", "Google", Some("8.8.8.8")),
                icmp("quad9", "Quad9", Some("9.9.9.9")),
                dns("resolver", "Gateway resolver", DnsVia::Gateway),
                dns("isp-dns", "ISP DNS", DnsVia::Upstreams),
                ProbeTarget {
                    id: "web".to_owned(),
                    name: "Web".to_owned(),
                    probe: Probe::Http {
                        url: "http://connectivitycheck.gstatic.com/generate_204".to_owned(),
                    },
                },
            ],
        }
    }
}

/// One thing the gateway probes; `id` names its series (`probe:<id>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WanProbeTarget"))]
pub struct ProbeTarget {
    /// `[a-z0-9-]`, 1–24 characters.
    pub id: String,
    pub name: String,
    pub probe: Probe,
}

impl ProbeTarget {
    fn validate(&self) -> Result<(), String> {
        let id = &self.id;
        if id.is_empty()
            || id.len() > ID_MAX
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(format!(
                "A target's id is 1–{ID_MAX} lowercase letters, digits and dashes: {id:?}"
            ));
        }
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX || name.contains(char::is_control) {
            return Err(format!("{id}: a name is 1–{NAME_MAX} characters"));
        }
        match &self.probe {
            Probe::Icmp { host: None } => Ok(()),
            Probe::Icmp { host: Some(host) } => {
                if host.parse::<IpAddr>().is_ok() || is_host_name(host) {
                    Ok(())
                } else {
                    Err(format!("{id}: {host:?} is not an address or a host name"))
                }
            }
            Probe::Dns { via, name } => {
                if !is_host_name(name) {
                    return Err(format!("{id}: {name:?} is not a name to look up"));
                }
                match via {
                    DnsVia::Server { address } if address.is_unspecified() => {
                        Err(format!("{id}: {address} is not a DNS server"))
                    }
                    _ => Ok(()),
                }
            }
            Probe::Http { url } => {
                let rest = url
                    .strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"));
                let host = rest.map(|rest| {
                    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    let authority = &rest[..end];
                    // A port is allowed; an address in brackets too.
                    authority
                        .rsplit_once(':')
                        .filter(|(_, port)| port.parse::<u16>().is_ok())
                        .map_or(authority, |(host, _)| host)
                });
                let host_ok = host.is_some_and(|host| {
                    is_host_name(host)
                        || host.parse::<IpAddr>().is_ok()
                        || host
                            .strip_prefix('[')
                            .and_then(|h| h.strip_suffix(']'))
                            .is_some_and(|h| h.parse::<IpAddr>().is_ok())
                });
                if url.len() > URL_MAX
                    || url.contains(|c: char| c.is_whitespace() || c.is_control())
                    || !host_ok
                {
                    Err(format!("{id}: {url:?} is not an http:// or https:// URL"))
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// A DNS name: dot-separated labels of letters, digits and dashes (not at a label's ends).
fn is_host_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && !name.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WanProbe"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Probe {
    /// Pings; no `host` pings the ISP's first hop (the default route's gateway).
    Icmp { host: Option<String> },
    /// Looks `name` up.
    Dns { via: DnsVia, name: String },
    /// Fetches `url`; any answer counts (a `204` URL is the lightest).
    Http { url: String },
}

/// Which server a DNS target asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WanDnsVia"))]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum DnsVia {
    /// The gateway's own resolver, as its clients ask it (and it asks its upstream).
    Gateway,
    /// Each server the WAN learned (`resolv.conf.auto`): a series each.
    Upstreams,
    Server {
        #[cfg_attr(feature = "ts", ts(as = "String"))]
        address: IpAddr,
    },
}

/// What one series' probes found in a round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WanProbeResult"))]
pub struct ProbeResult {
    /// The series key: the target's id, and `@<address>` for one of the WAN's DNS servers.
    pub key: String,
    pub sent: u32,
    /// The answers' times, in milliseconds, in the order they came.
    pub rtt_ms: Vec<f64>,
    /// Why nothing answered, when the probe says (`Network unreachable`, a timeout).
    pub error: Option<String>,
}

impl ProbeResult {
    pub fn answered(&self) -> bool {
        !self.rtt_ms.is_empty()
    }

    /// The share of probes lost, 0–100.
    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            return 100.0;
        }
        let lost = self
            .sent
            .saturating_sub(u32::try_from(self.rtt_ms.len()).unwrap_or(u32::MAX));
        f64::from(lost) * 100.0 / f64::from(self.sent)
    }

    /// The fastest, median and slowest answers.
    pub fn band(&self) -> Option<(f64, f64, f64)> {
        let mut sorted = self.rtt_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let (first, last) = (*sorted.first()?, *sorted.last()?);
        let middle = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            f64::midpoint(sorted[middle - 1], sorted[middle])
        } else {
            sorted[middle]
        };
        Some((first, median, last))
    }
}

/// One round of the gateway's probes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "WanProbeRound"))]
pub struct ProbeRound {
    pub at: Timestamp,
    /// The gateway has a default route.
    pub route: bool,
    /// The default route's gateway: the ISP's first hop.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub first_hop: Option<IpAddr>,
    pub results: Vec<ProbeResult>,
}

/// Where an outage lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WanFault {
    /// The gateway has no default route: its WAN is down.
    Gateway,
    /// The ISP's first hop doesn't answer either: the line, the ONT or the modem.
    Line,
    /// The first hop answers and nothing beyond it does: the ISP's network.
    Isp,
    /// Nothing says where: no first hop to ask.
    Unknown,
    /// The internet answers, but no DNS lookup does.
    Dns,
}

/// What a round says of the internet connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "state", content = "fault", rename_all = "snake_case")]
pub enum WanHealth {
    Up,
    Down(WanFault),
    /// No internet target to tell.
    Unknown,
}

/// What `round` says of the connection, as `settings` name its targets.
pub fn assess(settings: &WanMonitorSettings, round: &ProbeRound) -> WanHealth {
    let kind = |result: &&ProbeResult| settings.target(&result.key).map(|target| &target.probe);
    let internet: Vec<&ProbeResult> = round
        .results
        .iter()
        .filter(|result| {
            matches!(
                kind(result),
                Some(Probe::Icmp { host: Some(_) } | Probe::Http { .. })
            )
        })
        .collect();
    if internet.is_empty() {
        return WanHealth::Unknown;
    }
    if internet.iter().any(|result| result.answered()) {
        let dns: Vec<&ProbeResult> = round
            .results
            .iter()
            .filter(|result| matches!(kind(result), Some(Probe::Dns { .. })))
            .collect();
        if !dns.is_empty() && !dns.iter().any(|result| result.answered()) {
            return WanHealth::Down(WanFault::Dns);
        }
        return WanHealth::Up;
    }
    if !round.route {
        return WanHealth::Down(WanFault::Gateway);
    }
    let first_hop = round
        .results
        .iter()
        .find(|result| matches!(kind(result), Some(Probe::Icmp { host: None })))
        .filter(|_| round.first_hop.is_some());
    WanHealth::Down(match first_hop {
        Some(result) if result.answered() => WanFault::Isp,
        Some(_) => WanFault::Line,
        None => WanFault::Unknown,
    })
}

#[cfg(test)]
#[path = "wan_monitor_tests.rs"]
mod tests;

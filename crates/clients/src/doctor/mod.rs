//! The Connection Doctor (features/observability.md, Troubleshooting assistants; D-144): what
//! went on with one client over a span, as one timeline ([`TimelineEntry`]), and the issues found in it
//! ([`Issue`]), each with what points to it and the fixes to try ([`Fix`]).
//!
//! The timeline merges the client's events (joins, roams, steering, kicks), the DHCP exchanges
//! and the association trouble the nodes logged about it (`lines`), other log lines naming it,
//! and its failed DNS queries in 5-minute groups. [`diagnose`] is pure: the API gathers the
//! inputs from the store and the engine. The rules are in `rules.rs`.

mod lines;
mod rules;
#[cfg(test)]
mod tests;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{dns_queries::DnsQuery, events::EventRecord, log::LogRecord, wifi::Band};

pub use lines::{DhcpStep, WifiStep};

use crate::{Client, ClientSample};

/// The most entries a timeline holds: the newest are kept.
pub const TIMELINE_MAX: usize = 500;
/// How long a group of failed DNS queries spans.
const DNS_GROUP: Duration = Duration::from_mins(5);
/// The most domains a group names.
const DNS_GROUP_DOMAINS: usize = 5;

/// A WLAN as the doctor weighs it: the site's settings and how many access points carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WlanInfo {
    pub ssid: String,
    /// 802.11r is offered ([`uniwrt_model::site::Wlan::fast_roams`]).
    pub fast_roaming: bool,
    /// The site's steering applies to it.
    pub steering: bool,
    pub bands: Vec<Band>,
    /// The nodes that carry it.
    pub access_points: usize,
}

/// The client's DNS queries over the span, as the gateway's query log has them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DnsInput {
    /// Whether the query log is on (filtering's statistics, D-133); without it the counts are 0.
    pub logged: bool,
    pub queries: u64,
    pub blocked: u64,
    pub failed: u64,
    /// The failed queries, newest first (as many as the store gave).
    pub failures: Vec<DnsQuery>,
}

/// What the doctor reads.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub now: Timestamp,
    /// The span's start.
    pub from: Timestamp,
    pub client: &'a Client,
    /// Its recent samples, oldest first.
    pub samples: &'a [ClientSample],
    /// Its events over the span, in any order.
    pub events: &'a [EventRecord],
    /// The log lines that name it, in any order.
    pub logs: &'a [LogRecord],
    pub dns: &'a DnsInput,
    pub wlans: &'a [WlanInfo],
}

/// What the doctor found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Diagnosis {
    pub from: Timestamp,
    pub to: Timestamp,
    /// The worst first.
    pub issues: Vec<Issue>,
    /// Newest first, at most [`TIMELINE_MAX`].
    pub timeline: Vec<TimelineEntry>,
    /// Older entries were left out.
    pub truncated: bool,
    /// Whether the gateway logs DNS queries: without it, DNS can't be checked.
    pub dns_logged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "DoctorSeverity"))]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Critical,
    Warning,
    Info,
}

/// A detected issue: what it is, how bad, its last evidence and the fixes to try, best first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "DoctorIssue"))]
pub struct Issue {
    pub severity: IssueSeverity,
    pub problem: Problem,
    /// When it last showed.
    pub at: Timestamp,
    pub fixes: Vec<Fix>,
}

/// Why DHCP failed, as the latest failure shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DhcpFailure {
    /// The pool had no address left.
    PoolFull,
    /// The server refused the address the client asked for (a lease from elsewhere).
    Refused,
    /// The client asked and no address came within a minute.
    NoAnswer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "DoctorProblem"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    /// Its signal over the last 15 minutes, by the median.
    WeakSignal {
        signal_dbm: i16,
        node: Option<NodeId>,
        band: Option<Band>,
    },
    /// It holds on to a far access point while others carry its WLAN; `declined` counts the
    /// transition requests it turned down over the span.
    Sticky {
        signal_dbm: i16,
        node: Option<NodeId>,
        minutes: u32,
        declined: u32,
    },
    /// The key handshake failed again and again: most often a wrong passphrase. `connected`
    /// once it got on after the last failure.
    AuthFailures {
        failures: u32,
        ssid: Option<String>,
        connected: bool,
    },
    DhcpFailures {
        failures: u32,
        reason: DhcpFailure,
    },
    /// Online for `minutes` without an address the controller knows.
    NoAddress {
        minutes: u32,
    },
    DnsFailures {
        failed: u64,
        queries: u64,
    },
    /// Much of what it looks up is blocked: a blocklist may break one of its apps.
    DnsBlocked {
        blocked: u64,
        queries: u64,
    },
    /// It stays on 2.4 GHz with a signal good enough for 5 GHz, which it has used.
    StuckOn2g {
        signal_dbm: i16,
        ssid: Option<String>,
    },
    /// 802.11r is on for its WLAN, it never fast-roamed, and it fails to connect or stay.
    FastRoaming {
        ssid: String,
        short_sessions: u32,
        failures: u32,
    },
    /// It left `disconnects` times in the last hour.
    Flapping {
        disconnects: u32,
    },
}

/// A fix to try.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "DoctorFix"))]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// Bring it closer to the access point, or the access point to where it is used.
    MoveCloser {
        node: Option<NodeId>,
    },
    AddAccessPoint,
    /// Steering is off for its WLAN.
    TurnOnSteering {
        ssid: String,
    },
    CheckPassphrase {
        ssid: Option<String>,
    },
    /// The network's DHCP pool is full.
    WidenDhcpPool,
    /// Turn its Wi-Fi off and on (or forget the network and join again).
    Reconnect,
    CheckDhcpServer,
    /// The gateway's upstream resolvers (the WAN monitor shows how they answer).
    CheckUpstreamDns,
    /// Let it bypass DNS filtering.
    BypassFiltering,
    TurnOffFastRoaming {
        ssid: String,
    },
    /// Its driver or firmware.
    UpdateClient,
}

/// One thing on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelineEntry {
    Event {
        at: Timestamp,
        event: EventRecord,
    },
    Dhcp {
        at: Timestamp,
        node: Option<NodeId>,
        step: DhcpStep,
        interface: Option<String>,
        ip: Option<String>,
        detail: Option<String>,
    },
    Wifi {
        at: Timestamp,
        node: Option<NodeId>,
        step: WifiStep,
        interface: Option<String>,
        detail: String,
    },
    Log {
        at: Timestamp,
        node: Option<NodeId>,
        host: Option<String>,
        program: Option<String>,
        message: String,
    },
    /// Failed DNS queries within [`DNS_GROUP`] of `at`.
    Dns {
        at: Timestamp,
        failed: u32,
        domains: Vec<String>,
    },
}

impl TimelineEntry {
    pub const fn at(&self) -> Timestamp {
        match self {
            Self::Event { at, .. }
            | Self::Dhcp { at, .. }
            | Self::Wifi { at, .. }
            | Self::Log { at, .. }
            | Self::Dns { at, .. } => *at,
        }
    }
}

/// The client's timeline and issues.
pub fn diagnose(input: &Input<'_>) -> Diagnosis {
    let mut timeline = timeline(input);
    timeline.sort_by_key(|entry| std::cmp::Reverse(entry.at()));
    let mut issues = rules::issues(input, &timeline);
    issues.sort_by(|a, b| a.severity.cmp(&b.severity).then(b.at.cmp(&a.at)));
    let truncated = timeline.len() > TIMELINE_MAX;
    timeline.truncate(TIMELINE_MAX);
    Diagnosis {
        from: input.from,
        to: input.now,
        issues,
        timeline,
        truncated,
        dns_logged: input.dns.logged,
    }
}

/// Every entry of the span, unordered.
fn timeline(input: &Input<'_>) -> Vec<TimelineEntry> {
    let in_span = |at: Timestamp| at >= input.from && at <= input.now;
    let mut entries: Vec<TimelineEntry> = input
        .events
        .iter()
        .filter(|event| in_span(event.at))
        .map(|event| TimelineEntry::Event {
            at: event.at,
            event: event.clone(),
        })
        .collect();
    for record in input.logs {
        let at = record.entry.time;
        if !in_span(at) {
            continue;
        }
        let Some(line) = lines::read(record, input.client.mac) else {
            continue;
        };
        let node = record.node;
        entries.push(match line {
            lines::Line::Dhcp {
                step,
                interface,
                ip,
                detail,
            } => TimelineEntry::Dhcp {
                at,
                node,
                step,
                interface,
                ip,
                detail,
            },
            lines::Line::Wifi {
                step,
                interface,
                detail,
            } => TimelineEntry::Wifi {
                at,
                node,
                step,
                interface,
                detail,
            },
            lines::Line::Routine => continue,
            lines::Line::Other => TimelineEntry::Log {
                at,
                node,
                host: record.host.clone(),
                program: record.entry.program.clone(),
                message: record.entry.message.clone(),
            },
        });
    }
    entries.extend(dns_groups(&input.dns.failures, input.from, input.now));
    entries
}

/// Failed queries grouped: a group starts at its oldest query and takes those within
/// [`DNS_GROUP`] of it.
fn dns_groups(failures: &[DnsQuery], from: Timestamp, now: Timestamp) -> Vec<TimelineEntry> {
    let mut queries: Vec<&DnsQuery> = failures
        .iter()
        .filter(|query| query.at >= from && query.at <= now)
        .collect();
    queries.sort_by_key(|query| query.at);
    let mut groups = Vec::new();
    let mut rest = queries.as_slice();
    while let Some(first) = rest.first() {
        let end = first.at.saturating_add(DNS_GROUP);
        let size = rest.iter().take_while(|query| query.at < end).count();
        let mut domains: Vec<String> = Vec::new();
        for query in &rest[..size] {
            if domains.len() < DNS_GROUP_DOMAINS && !domains.contains(&query.domain) {
                domains.push(query.domain.clone());
            }
        }
        groups.push(TimelineEntry::Dns {
            at: first.at,
            failed: u32::try_from(size).unwrap_or(u32::MAX),
            domains,
        });
        rest = &rest[size..];
    }
    groups
}

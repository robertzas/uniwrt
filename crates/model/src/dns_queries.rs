//! DNS query statistics (features/network.md, Query statistics; D-133): dnsmasq's query log as
//! the gateway writes it while filtering's statistics are on, read line by line ([`parse`]),
//! each query paired with how it was answered ([`Pairing`]), and the [`DnsQuery`] records the
//! controller keeps.
//!
//! OpenWrt passes `logqueries` as `--log-queries=extra`, so every line of a query starts with
//! its serial and the client's address and port (`7 192.168.1.20/53311 query[A] example.com
//! from 192.168.1.20`); the plain form, without them, is read too. dnsmasq answers from its
//! configuration, its cache or its hosts files at once, logging the answer right after the
//! query; a query it forwards logs `forwarded`, and the upstream's reply comes later. A blocked
//! name (`server=/name/` without a server) logs `config <name> is NXDOMAIN`, as an unknown name
//! under the local domain and a single label do: [`is_blocked`] tells them apart.

use std::{collections::HashMap, hash::Hash, net::IpAddr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{MacAddr, Timestamp};

/// How dnsmasq answered a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, rename = "DnsOutcome"))]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Sent upstream (or joined a query already sent).
    Forwarded,
    /// From the cache.
    Cached,
    /// From the gateway's own names: hosts files, DHCP leases, configured addresses.
    Local,
    /// By a blocklist.
    Blocked,
    /// Refused or failed (no upstream answering).
    Failed,
}

/// One client's query as the controller keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DnsQuery {
    pub at: Timestamp,
    /// The address the query came from.
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub client: IpAddr,
    /// The client's MAC, where the controller knows who holds the address.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub mac: Option<MacAddr>,
    /// `A`, `AAAA`, `HTTPS`, … as dnsmasq names it.
    pub qtype: String,
    pub domain: String,
    pub outcome: Outcome,
}

impl DnsQuery {
    /// Who asked, as the statistics count clients: the MAC where known, else the address.
    pub fn client_key(&self) -> String {
        self.mac
            .map_or_else(|| self.client.to_string(), |mac| mac.to_string())
    }
}

/// One client's queries in one hour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DnsHour {
    /// The hour's start.
    pub hour: Timestamp,
    /// [`DnsQuery::client_key`].
    pub client: String,
    pub queries: u64,
    pub blocked: u64,
}

/// The queries and blocked queries of one hour, summed over clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DnsHourTotal {
    /// The hour's start.
    pub hour: Timestamp,
    pub queries: u64,
    pub blocked: u64,
}

/// One client's queries and blocked queries over a span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsClientTotal {
    /// [`DnsQuery::client_key`].
    pub client: String,
    pub queries: u64,
    pub blocked: u64,
}

/// How often a domain was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DomainCount {
    pub domain: String,
    pub queries: u64,
}

/// What a query-log line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    Query {
        serial: Option<u64>,
        qtype: &'a str,
        name: &'a str,
        from: IpAddr,
    },
    Answer {
        serial: Option<u64>,
        name: &'a str,
        answer: Answer,
    },
    /// A line of the query log the statistics don't use (upstream replies, DNSSEC, sets).
    Other,
}

/// What an answer line says, before [`is_blocked`] reads an NXDOMAIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Forwarded,
    Cached,
    Local,
    /// NXDOMAIN from the configuration: a blocked name, or an unknown local one.
    Nxdomain,
    Failed,
}

/// Reads a line `program` logged as dnsmasq's query log; `None` for any other line (which
/// stays a log entry).
pub fn parse<'a>(program: Option<&str>, message: &'a str) -> Option<Line<'a>> {
    if program != Some("dnsmasq") {
        return None;
    }
    let (serial, rest) = match extra(message) {
        Some((serial, rest)) => (Some(serial), rest),
        None => (None, message),
    };
    let mut words = rest.split(' ');
    let first = words.next()?;
    let line = if let Some(qtype) = first
        .strip_prefix("query[")
        .and_then(|t| t.strip_suffix(']'))
    {
        let (name, from, ip) = (words.next()?, words.next()?, words.next()?);
        if from != "from" {
            return serial.map(|_| Line::Other);
        }
        let Ok(from) = ip.parse() else {
            return serial.map(|_| Line::Other);
        };
        Line::Query {
            serial,
            qtype,
            name,
            from,
        }
    } else {
        let (name, verb, value) = (words.next(), words.next(), words.next());
        let answer = match (first, verb) {
            ("forwarded", Some("to")) => Some(Answer::Forwarded),
            ("cached" | "cached-stale", Some("is")) => Some(Answer::Cached),
            ("config", Some("is")) => Some(match value.unwrap_or_default() {
                "NXDOMAIN" => Answer::Nxdomain,
                v if v.starts_with("REFUSED") || v.starts_with("SERVFAIL") => Answer::Failed,
                _ => Answer::Local,
            }),
            ("DHCP", Some("is")) => Some(Answer::Local),
            (path, Some("is")) if path.starts_with('/') => Some(Answer::Local),
            _ => None,
        };
        match (answer, name) {
            (Some(answer), Some(name)) => Line::Answer {
                serial,
                name,
                answer,
            },
            _ if serial.is_some() || is_other(first) => Line::Other,
            _ => return None,
        }
    };
    Some(line)
}

/// The extra form's `<serial> <address>/<port> ` and the rest of the line.
fn extra(message: &str) -> Option<(u64, &str)> {
    let (serial, rest) = message.split_once(' ')?;
    let (client, rest) = rest.split_once(' ')?;
    let (address, port) = client.rsplit_once('/')?;
    let serial = serial.parse().ok()?;
    (port.parse::<u16>().is_ok() && address.parse::<IpAddr>().is_ok()).then_some((serial, rest))
}

/// The plain form's other query-log lines.
fn is_other(first: &str) -> bool {
    matches!(first, "reply" | "validation" | "ipset" | "nftset")
        || first.starts_with("dnssec-query[")
        || first.starts_with("dnssec-retry[")
}

/// Whether an NXDOMAIN from dnsmasq's configuration was a blocklist's: not a reverse lookup,
/// not a single label (`domainneeded`) and not a name under the site's `local_domain`.
pub fn is_blocked(qtype: &str, name: &str, local_domain: &str) -> bool {
    let name = name.trim_end_matches('.');
    let local = !local_domain.is_empty()
        && (name.eq_ignore_ascii_case(local_domain)
            || name.len() > local_domain.len()
                && name.as_bytes()[name.len() - local_domain.len() - 1] == b'.'
                && name[name.len() - local_domain.len()..].eq_ignore_ascii_case(local_domain));
    qtype != "PTR" && name.contains('.') && name.parse::<IpAddr>().is_err() && !local
}

/// How far below the highest serial seen a query counts as one seen again (a backfill), not a
/// dnsmasq restarted under the same pid.
const REPEATS: u64 = 100_000;

/// A query waiting for its answer line.
#[derive(Debug, Clone)]
struct Pending {
    at: Timestamp,
    client: IpAddr,
    qtype: String,
    domain: String,
    serial: Option<u64>,
}

impl Pending {
    fn answered(self, outcome: Outcome) -> DnsQuery {
        DnsQuery {
            at: self.at,
            client: self.client,
            mac: None,
            qtype: self.qtype,
            domain: self.domain,
            outcome,
        }
    }
}

#[derive(Debug, Default)]
struct Process {
    highest: Option<u64>,
    pending: Option<Pending>,
}

/// Pairs each dnsmasq process's queries with their answers. `K` names a process (a node and a
/// pid). A query whose answer never comes was forwarded (or joined one forwarded): it is
/// complete when the process's next query arrives, or at [`Pairing::expire`]. A query the
/// extra form numbers at or below the highest serial seen was seen before, and is skipped.
#[derive(Debug)]
pub struct Pairing<K> {
    processes: HashMap<K, Process>,
    local_domain: String,
}

impl<K: Eq + Hash + Clone> Pairing<K> {
    pub fn new(local_domain: &str) -> Self {
        Self {
            processes: HashMap::new(),
            local_domain: local_domain.to_ascii_lowercase(),
        }
    }

    /// The site's local domain changed.
    pub fn set_local_domain(&mut self, local_domain: &str) {
        local_domain
            .to_ascii_lowercase()
            .clone_into(&mut self.local_domain);
    }

    /// Takes a line `process` logged at `at`; returns the queries it completes (at most two:
    /// one left unanswered, and one answered).
    pub fn line(&mut self, process: &K, at: Timestamp, line: Line<'_>) -> Vec<DnsQuery> {
        let mut done = Vec::new();
        match line {
            Line::Query {
                serial,
                qtype,
                name,
                from,
            } => {
                let state = self.processes.entry(process.clone()).or_default();
                if let Some(pending) = state.pending.take() {
                    done.push(pending.answered(Outcome::Forwarded));
                }
                if let Some(serial) = serial {
                    match state.highest {
                        Some(top) if serial <= top && top - serial < REPEATS => return done,
                        _ => state.highest = Some(serial),
                    }
                }
                state.pending = Some(Pending {
                    at,
                    client: from,
                    qtype: qtype.to_owned(),
                    domain: name.trim_end_matches('.').to_ascii_lowercase(),
                    serial,
                });
            }
            Line::Answer {
                serial,
                name,
                answer,
            } => {
                let Some(state) = self.processes.get_mut(process) else {
                    return done;
                };
                let matches = state
                    .pending
                    .as_ref()
                    .is_some_and(|p| match (serial, p.serial) {
                        (Some(serial), Some(pending)) => serial == pending,
                        // A reverse lookup's answer names the address.
                        _ => {
                            p.qtype == "PTR"
                                || p.domain.eq_ignore_ascii_case(name.trim_end_matches('.'))
                        }
                    });
                if let Some(pending) = state.pending.take_if(|_| matches) {
                    let outcome = match answer {
                        Answer::Forwarded => Outcome::Forwarded,
                        Answer::Cached => Outcome::Cached,
                        Answer::Failed => Outcome::Failed,
                        Answer::Nxdomain
                            if is_blocked(&pending.qtype, &pending.domain, &self.local_domain) =>
                        {
                            Outcome::Blocked
                        }
                        Answer::Local | Answer::Nxdomain => Outcome::Local,
                    };
                    done.push(pending.answered(outcome));
                }
            }
            Line::Other => {}
        }
        done
    }

    /// Completes the queries waiting since before `before` as forwarded.
    pub fn expire(&mut self, before: Timestamp) -> Vec<DnsQuery> {
        let mut done = Vec::new();
        for state in self.processes.values_mut() {
            if let Some(pending) = state.pending.take_if(|p| p.at < before) {
                done.push(pending.answered(Outcome::Forwarded));
            }
        }
        done
    }

    /// Forgets the processes `keep` refuses (a node removed).
    pub fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.processes.retain(|k, _| keep(k));
    }
}

#[cfg(test)]
#[path = "dns_queries_tests.rs"]
mod tests;

//! The doctor's rules: each looks at the inputs and the timeline and may add an issue.
//!
//! | Issue | When | IssueSeverity |
//! |---|---|---|
//! | Weak signal | online over Wi-Fi, the median of the last 15 minutes' signal (3 samples or more) below −75 dBm | critical below −82 dBm, else warning |
//! | Sticky | the same, connected there for 10 minutes or more, and 2 or more access points carry its WLAN (instead of weak signal) | as weak signal |
//! | Passphrase | 3 or more failed key handshakes over the span | critical while it hasn't got on since, else warning |
//! | DHCP | 3 or more failed exchanges (a refusal, no address left, or no address within a minute of asking), or the latest exchange failed | critical while the latest failed, else warning |
//! | No address | online over Wi-Fi for 5 minutes or more without an address the controller knows | warning |
//! | DNS failures | 20 or more queries, 5 or more failed, 10 % or more failed | critical from 30 %, else warning |
//! | DNS blocked | 50 or more blocked, 25 % or more of its queries | info |
//! | Stuck on 2.4 GHz | on 2.4 GHz for 10 minutes or more, median signal −65 dBm or better, it has used 5 or 6 GHz (or is Wi-Fi 5), and its WLAN has 5 GHz | info |
//! | 802.11r | its WLAN offers 802.11r, it never fast-roamed, and a fast transition failed or 3 or more sessions ended within a minute | warning |
//! | Flapping | 6 or more disconnects in the last hour | warning |

use std::time::Duration;

use uniwrt_core::Timestamp;
use uniwrt_model::{
    client::ClientMedium,
    event::SteerAction,
    events::EventKind,
    facts::StationCapability,
    wifi::{Band, WifiStandard},
};

use super::{
    DhcpFailure, DhcpStep, Fix, Input, Issue, IssueSeverity, Problem, TimelineEntry, WifiStep,
    WlanInfo,
};
use crate::Connection;

const RECENT: Duration = Duration::from_mins(15);
const MIN_SAMPLES: usize = 3;
const WEAK_DBM: i16 = -75;
const VERY_WEAK_DBM: i16 = -82;
const STRONG_DBM: i16 = -65;
/// How long a client stays at a place before staying counts against it.
const SETTLED: Duration = Duration::from_mins(10);
const AUTH_FAILURES: u32 = 3;
const DHCP_FAILURES: u32 = 3;
/// How long an address may take after a discover or request.
const DHCP_ANSWER: Duration = Duration::from_mins(1);
const NO_ADDRESS: Duration = Duration::from_mins(5);
const DNS_MIN_QUERIES: u64 = 20;
const DNS_MIN_FAILED: u64 = 5;
const DNS_FAILED_PERCENT: u64 = 10;
const DNS_FAILED_CRITICAL_PERCENT: u64 = 30;
const DNS_MIN_BLOCKED: u64 = 50;
const DNS_BLOCKED_PERCENT: u64 = 25;
const SHORT_SESSION_MS: u64 = 60_000;
const SHORT_SESSIONS: u32 = 3;
const FLAPPING_SPAN: Duration = Duration::from_hours(1);
const FLAPPING: u32 = 6;

pub(super) fn issues(input: &Input<'_>, timeline: &[TimelineEntry]) -> Vec<Issue> {
    let mut issues = Vec::new();
    signal(input, &mut issues);
    auth(input, timeline, &mut issues);
    dhcp(input, timeline, &mut issues);
    address(input, &mut issues);
    dns(input, &mut issues);
    band(input, &mut issues);
    fast_roaming(input, timeline, &mut issues);
    flapping(input, &mut issues);
    issues
}

/// Where it is connected over Wi-Fi now.
fn on_wifi<'a>(input: &Input<'a>) -> Option<&'a Connection> {
    let client = input.client;
    client
        .connection
        .as_ref()
        .filter(|connection| client.online && connection.medium == ClientMedium::Wifi)
}

fn wlan<'a>(input: &Input<'a>, ssid: Option<&str>) -> Option<&'a WlanInfo> {
    let ssid = ssid?;
    input.wlans.iter().find(|wlan| wlan.ssid == ssid)
}

/// The median signal of the last 15 minutes, when there are enough samples.
fn recent_signal(input: &Input<'_>) -> Option<i16> {
    let since = input.now.saturating_sub(RECENT);
    let mut values: Vec<i16> = input
        .samples
        .iter()
        .filter(|sample| sample.at >= since)
        .filter_map(|sample| sample.signal_dbm)
        .collect();
    if values.len() < MIN_SAMPLES {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

fn minutes(duration: Duration) -> u32 {
    u32::try_from(duration.as_secs() / 60).unwrap_or(u32::MAX)
}

fn signal(input: &Input<'_>, issues: &mut Vec<Issue>) {
    let Some(connection) = on_wifi(input) else {
        return;
    };
    let Some(signal_dbm) = recent_signal(input).filter(|dbm| *dbm < WEAK_DBM) else {
        return;
    };
    let severity = if signal_dbm < VERY_WEAK_DBM {
        IssueSeverity::Critical
    } else {
        IssueSeverity::Warning
    };
    let at = input.samples.last().map_or(input.now, |sample| sample.at);
    let node = connection.node;
    let stayed = input.now.saturating_duration_since(connection.since);
    let wlan = wlan(input, connection.ssid.as_deref());
    if stayed >= SETTLED && wlan.is_some_and(|wlan| wlan.access_points >= 2) {
        let declined = input
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event.what,
                    EventKind::ClientSteered {
                        action: SteerAction::TransitionRejected,
                        ..
                    }
                )
            })
            .map(|event| event.count)
            .sum();
        let mut fixes = Vec::new();
        if let Some(wlan) = wlan.filter(|wlan| !wlan.steering) {
            fixes.push(Fix::TurnOnSteering {
                ssid: wlan.ssid.clone(),
            });
        }
        fixes.extend([Fix::Reconnect, Fix::MoveCloser { node }]);
        issues.push(Issue {
            severity,
            problem: Problem::Sticky {
                signal_dbm,
                node,
                minutes: minutes(stayed),
                declined,
            },
            at,
            fixes,
        });
    } else {
        issues.push(Issue {
            severity,
            problem: Problem::WeakSignal {
                signal_dbm,
                node,
                band: connection.band,
            },
            at,
            fixes: vec![Fix::MoveCloser { node }, Fix::AddAccessPoint],
        });
    }
}

/// When it got on last: a join or a roam, or its current connection.
fn last_on(input: &Input<'_>) -> Option<Timestamp> {
    let joined = input
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.what,
                EventKind::ClientJoined { .. } | EventKind::ClientRoamed { .. }
            )
        })
        .map(|event| event.last_at)
        .max();
    let connected = input
        .client
        .connection
        .as_ref()
        .filter(|_| input.client.online)
        .map(|connection| connection.since);
    joined.max(connected)
}

fn wifi_steps(timeline: &[TimelineEntry], wanted: &[WifiStep]) -> (u32, Option<Timestamp>) {
    timeline
        .iter()
        .fold((0, None), |(count, last), entry| match entry {
            TimelineEntry::Wifi { at, step, .. } if wanted.contains(step) => {
                (count + 1, last.max(Some(*at)))
            }
            _ => (count, last),
        })
}

fn auth(input: &Input<'_>, timeline: &[TimelineEntry], issues: &mut Vec<Issue>) {
    let (failures, last) = wifi_steps(timeline, &[WifiStep::AuthFailed]);
    let Some(last) = last.filter(|_| failures >= AUTH_FAILURES) else {
        return;
    };
    let connected = last_on(input).is_some_and(|on| on > last);
    let ssid = input
        .client
        .connection
        .as_ref()
        .and_then(|connection| connection.ssid.clone());
    issues.push(Issue {
        severity: if connected {
            IssueSeverity::Warning
        } else {
            IssueSeverity::Critical
        },
        problem: Problem::AuthFailures {
            failures,
            ssid: ssid.clone(),
            connected,
        },
        at: last,
        fixes: vec![Fix::CheckPassphrase { ssid }],
    });
}

fn dhcp(input: &Input<'_>, timeline: &[TimelineEntry], issues: &mut Vec<Issue>) {
    let mut failures = 0_u32;
    let mut last_failure: Option<(Timestamp, DhcpFailure)> = None;
    let mut last_ack: Option<Timestamp> = None;
    let mut asked: Option<Timestamp> = None;
    let unanswered = |asked: &mut Option<Timestamp>, now: Timestamp, failures: &mut u32| {
        let since = (*asked)?;
        if now.saturating_duration_since(since) <= DHCP_ANSWER {
            return None;
        }
        *asked = None;
        *failures += 1;
        Some((since, DhcpFailure::NoAnswer))
    };
    for entry in timeline.iter().rev() {
        let TimelineEntry::Dhcp { at, step, .. } = entry else {
            continue;
        };
        if let Some(failure) = unanswered(&mut asked, *at, &mut failures) {
            last_failure = Some(failure);
        }
        match step {
            DhcpStep::Discover | DhcpStep::Request => {
                asked.get_or_insert(*at);
            }
            DhcpStep::Ack => {
                asked = None;
                last_ack = Some(*at);
            }
            DhcpStep::Nak | DhcpStep::NoAddress => {
                asked = None;
                failures += 1;
                let reason = if *step == DhcpStep::Nak {
                    DhcpFailure::Refused
                } else {
                    DhcpFailure::PoolFull
                };
                last_failure = Some((*at, reason));
            }
            DhcpStep::Offer | DhcpStep::Decline | DhcpStep::Release | DhcpStep::Inform => {}
        }
    }
    if let Some(failure) = unanswered(&mut asked, input.now, &mut failures) {
        last_failure = Some(failure);
    }
    let Some((at, reason)) = last_failure else {
        return;
    };
    let failing = last_ack.is_none_or(|ack| ack < at);
    if failures < DHCP_FAILURES && !failing {
        return;
    }
    let fixes = match reason {
        DhcpFailure::PoolFull => vec![Fix::WidenDhcpPool],
        DhcpFailure::Refused => vec![Fix::Reconnect],
        DhcpFailure::NoAnswer => vec![Fix::CheckDhcpServer, Fix::Reconnect],
    };
    issues.push(Issue {
        severity: if failing {
            IssueSeverity::Critical
        } else {
            IssueSeverity::Warning
        },
        problem: Problem::DhcpFailures { failures, reason },
        at,
        fixes,
    });
}

fn address(input: &Input<'_>, issues: &mut Vec<Issue>) {
    let Some(connection) = on_wifi(input) else {
        return;
    };
    let online = input.now.saturating_duration_since(connection.since);
    if !input.client.ips.is_empty() || online < NO_ADDRESS {
        return;
    }
    issues.push(Issue {
        severity: IssueSeverity::Warning,
        problem: Problem::NoAddress {
            minutes: minutes(online),
        },
        at: input.now,
        fixes: vec![Fix::CheckDhcpServer, Fix::Reconnect],
    });
}

fn dns(input: &Input<'_>, issues: &mut Vec<Issue>) {
    let dns = input.dns;
    if !dns.logged || dns.queries < DNS_MIN_QUERIES {
        return;
    }
    let percent = |count: u64| count.saturating_mul(100) / dns.queries;
    if dns.failed >= DNS_MIN_FAILED && percent(dns.failed) >= DNS_FAILED_PERCENT {
        issues.push(Issue {
            severity: if percent(dns.failed) >= DNS_FAILED_CRITICAL_PERCENT {
                IssueSeverity::Critical
            } else {
                IssueSeverity::Warning
            },
            problem: Problem::DnsFailures {
                failed: dns.failed,
                queries: dns.queries,
            },
            at: dns.failures.first().map_or(input.now, |query| query.at),
            fixes: vec![Fix::CheckUpstreamDns],
        });
    }
    if dns.blocked >= DNS_MIN_BLOCKED && percent(dns.blocked) >= DNS_BLOCKED_PERCENT {
        issues.push(Issue {
            severity: IssueSeverity::Info,
            problem: Problem::DnsBlocked {
                blocked: dns.blocked,
                queries: dns.queries,
            },
            at: input.now,
            fixes: vec![Fix::BypassFiltering],
        });
    }
}

fn band(input: &Input<'_>, issues: &mut Vec<Issue>) {
    let Some(connection) = on_wifi(input) else {
        return;
    };
    let wifi = &input.client.wifi;
    let dual = wifi.bands.contains(&Band::G5)
        || wifi.bands.contains(&Band::G6)
        || wifi.standard == Some(WifiStandard::Vht);
    let wlan = wlan(input, connection.ssid.as_deref());
    if connection.band != Some(Band::G2)
        || !dual
        || input.now.saturating_duration_since(connection.since) < SETTLED
        || !wlan.is_some_and(|wlan| wlan.bands.contains(&Band::G5))
    {
        return;
    }
    let Some(signal_dbm) = recent_signal(input).filter(|dbm| *dbm >= STRONG_DBM) else {
        return;
    };
    let mut fixes = Vec::new();
    if let Some(wlan) = wlan.filter(|wlan| !wlan.steering) {
        fixes.push(Fix::TurnOnSteering {
            ssid: wlan.ssid.clone(),
        });
    }
    fixes.push(Fix::Reconnect);
    issues.push(Issue {
        severity: IssueSeverity::Info,
        problem: Problem::StuckOn2g {
            signal_dbm,
            ssid: connection.ssid.clone(),
        },
        at: input.samples.last().map_or(input.now, |sample| sample.at),
        fixes,
    });
}

/// The SSID it is on, or joined last over the span.
fn ssid(input: &Input<'_>) -> Option<String> {
    on_wifi(input)
        .and_then(|connection| connection.ssid.clone())
        .or_else(|| {
            input
                .events
                .iter()
                .filter_map(|event| match &event.what {
                    EventKind::ClientJoined {
                        ssid: Some(ssid), ..
                    } => Some((event.at, ssid)),
                    _ => None,
                })
                .max_by_key(|(at, _)| *at)
                .map(|(_, ssid)| ssid.clone())
        })
}

fn fast_roaming(input: &Input<'_>, timeline: &[TimelineEntry], issues: &mut Vec<Issue>) {
    let Some(ssid) = ssid(input) else {
        return;
    };
    if !wlan(input, Some(&ssid)).is_some_and(|wlan| wlan.fast_roaming)
        || input
            .client
            .wifi
            .capabilities
            .contains(&StationCapability::FastTransition)
    {
        return;
    }
    let short = input.events.iter().filter_map(|event| match event.what {
        EventKind::ClientLeft {
            connected_ms: Some(ms),
        } if ms < SHORT_SESSION_MS => Some(event.at),
        _ => None,
    });
    let (short_sessions, last_short) = short.fold((0_u32, None), |(count, last), at| {
        (count + 1, last.max(Some(at)))
    });
    let (ft_failed, last_ft) = wifi_steps(timeline, &[WifiStep::FastTransitionFailed]);
    if ft_failed == 0 && short_sessions < SHORT_SESSIONS {
        return;
    }
    let (failures, last_failure) = wifi_steps(
        timeline,
        &[WifiStep::FastTransitionFailed, WifiStep::AuthFailed],
    );
    let at = last_short
        .max(last_ft)
        .max(last_failure)
        .unwrap_or(input.now);
    issues.push(Issue {
        severity: IssueSeverity::Warning,
        problem: Problem::FastRoaming {
            ssid: ssid.clone(),
            short_sessions,
            failures,
        },
        at,
        fixes: vec![Fix::TurnOffFastRoaming { ssid }, Fix::UpdateClient],
    });
}

fn flapping(input: &Input<'_>, issues: &mut Vec<Issue>) {
    let since = input.now.saturating_sub(FLAPPING_SPAN);
    let left = input
        .events
        .iter()
        .filter(|event| event.at >= since && matches!(event.what, EventKind::ClientLeft { .. }));
    let (disconnects, last) = left.fold((0_u32, None), |(count, last), event| {
        (
            count.saturating_add(event.count),
            last.max(Some(event.last_at)),
        )
    });
    let Some(at) = last.filter(|_| disconnects >= FLAPPING) else {
        return;
    };
    issues.push(Issue {
        severity: IssueSeverity::Warning,
        problem: Problem::Flapping { disconnects },
        at,
        fixes: vec![Fix::Reconnect, Fix::UpdateClient],
    });
}

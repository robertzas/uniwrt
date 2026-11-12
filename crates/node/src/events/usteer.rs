//! usteer's notifications from its `usteer` object (usteer 2022.08, which both 24.10 and 25.12
//! ship): `probe_req_deny`, `auth_req_deny`, `assoc_req_deny`, `signal_kick` and
//! `load_kick_client`, each with the access point as usteer names it (`node`, such as
//! `hostapd.phy1-ap0`), the client (`sta`; signal kicks have none), its `signal`, the `reason`,
//! the `threshold` it was held to (`[current, limit]`) and the better access point (`remote`,
//! `<host>#hostapd.<ifname>` for another node's). The agent drops the accepts and the load
//! kick's other steps; other types are not modelled.

use std::net::IpAddr;

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::event::{NodeEvent, SteerAction, SteerEvent, SteerReason, SteerTarget};

use super::decode;

#[derive(Deserialize)]
struct Raw {
    node: String,
    sta: Option<MacAddr>,
    signal: Option<i64>,
    reason: Option<String>,
    #[serde(default)]
    threshold: Vec<i64>,
    remote: Option<Remote>,
}

#[derive(Deserialize)]
struct Remote {
    name: String,
    signal: Option<i64>,
}

pub(super) fn parse(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
    let action = match kind {
        "probe_req_deny" => SteerAction::ProbeDenied,
        "auth_req_deny" => SteerAction::AuthDenied,
        "assoc_req_deny" => SteerAction::AssocDenied,
        "signal_kick" => SteerAction::SignalKick,
        "load_kick_client" => SteerAction::LoadKick,
        _ => return Ok(None),
    };
    let raw: Raw = decode(raw)?;
    let reason = raw.reason.as_deref().and_then(|reason| match reason {
        "low_signal" => Some(SteerReason::LowSignal),
        "connect_delay" => Some(SteerReason::ConnectDelay),
        "better_candidate" => Some(SteerReason::BetterCandidate),
        "retry_exceeded" => Some(SteerReason::RetryExceeded),
        _ => None,
    });
    let threshold = |i: usize| raw.threshold.get(i).copied().and_then(dbm);
    // A signal kick holds the client's signal against the minimum, as a refusal for low
    // signal does; other thresholds are counts or times.
    let kick = action == SteerAction::SignalKick;
    let min_signal_dbm = (kick || reason == Some(SteerReason::LowSignal))
        .then(|| threshold(1))
        .flatten();
    let signal_dbm = raw
        .signal
        .and_then(dbm)
        .or_else(|| kick.then(|| threshold(0)).flatten());
    Ok(Some(NodeEvent::Steer(SteerEvent {
        mac: raw.sta,
        iface: iface(&raw.node).to_owned(),
        action,
        reason,
        signal_dbm,
        min_signal_dbm,
        target: raw.remote.map(|remote| target(&remote)),
        status: None,
    })))
}

/// `hostapd.phy1-ap0` → `phy1-ap0`.
fn iface(name: &str) -> &str {
    name.strip_prefix("hostapd.").unwrap_or(name)
}

/// `192.168.1.2#hostapd.phy1-ap0` (another node's), or `hostapd.phy0-ap0` (this node's).
fn target(remote: &Remote) -> SteerTarget {
    let (host, name) = match remote.name.split_once('#') {
        Some((host, name)) => (host.parse::<IpAddr>().ok(), name),
        None => (None, remote.name.as_str()),
    };
    SteerTarget::Usteer {
        host,
        iface: iface(name).to_owned(),
        signal_dbm: remote.signal.and_then(dbm),
    }
}

/// A signal in dBm: usteer sends signed values as blobmsg u32, which a reader may decode as
/// unsigned.
fn dbm(value: i64) -> Option<i16> {
    let value = if value > i64::from(i32::MAX) {
        i64::from(u32::try_from(value).ok()?.cast_signed())
    } else {
        value
    };
    i16::try_from(value).ok()
}

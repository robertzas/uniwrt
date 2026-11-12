//! hostapd's notifications from each BSS's `hostapd.<ifname>` object: `auth` and `assoc` with
//! the request's signal and frequency, `sta-authorized` with how the station authenticated,
//! `disassoc`, and `bss-transition-response` (a client's answer to a BSS transition request,
//! with its `status-code` and the `target-bssid` it goes to, as the agent reads them: the
//! status a boolean). All carry the station's `address`
//! and the BSS's `ifname`.

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::event::{
    NodeEvent, StationChange, StationEvent, SteerAction, SteerEvent, SteerTarget,
};

use super::decode;

#[derive(Deserialize)]
struct Raw {
    address: MacAddr,
    ifname: String,
    signal: Option<i16>,
    freq: Option<u32>,
    #[serde(rename = "auth-alg")]
    auth_alg: Option<String>,
}

#[derive(Deserialize)]
struct Answer {
    address: MacAddr,
    ifname: String,
    #[serde(rename = "status-code")]
    status: Status,
    #[serde(rename = "target-bssid")]
    target: Option<MacAddr>,
}

/// hostapd sends the status as a blobmsg u8, which ucode's ubus module reads as a boolean:
/// `false` accepts, `true` is any refusal. A reader that keeps numbers gets the code.
#[derive(Deserialize)]
#[serde(untagged)]
enum Status {
    Refused(bool),
    Code(u8),
}

pub(super) fn parse(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
    if kind == "bss-transition-response" {
        let answer: Answer = decode(raw)?;
        let (accepted, status) = match answer.status {
            Status::Refused(refused) => (!refused, None),
            Status::Code(code) => (code == 0, Some(code)),
        };
        return Ok(Some(NodeEvent::Steer(SteerEvent {
            mac: Some(answer.address),
            iface: answer.ifname,
            action: if accepted {
                SteerAction::TransitionAccepted
            } else {
                SteerAction::TransitionRejected
            },
            reason: None,
            signal_dbm: None,
            min_signal_dbm: None,
            target: answer.target.map(|bssid| SteerTarget::Bssid { bssid }),
            status,
        })));
    }
    let change = match kind {
        "auth" => StationChange::AuthRequest,
        "assoc" => StationChange::AssocRequest,
        "sta-authorized" => StationChange::Authorized,
        "disassoc" => StationChange::Disassociated,
        _ => return Ok(None),
    };
    let raw: Raw = decode(raw)?;
    Ok(Some(NodeEvent::Station(StationEvent {
        mac: raw.address,
        iface: raw.ifname,
        change,
        signal_dbm: raw.signal,
        freq_mhz: raw.freq,
        auth_alg: raw.auth_alg,
    })))
}

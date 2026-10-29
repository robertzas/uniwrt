//! hostapd's notifications from each BSS's `hostapd.<ifname>` object: `auth` and `assoc` with
//! the request's signal and frequency, `sta-authorized` with how the station authenticated, and
//! `disassoc`. All carry the station's `address` and the BSS's `ifname`.

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::event::{NodeEvent, StationChange, StationEvent};

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

pub(super) fn parse(kind: &str, raw: &str) -> Result<Option<NodeEvent>, String> {
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

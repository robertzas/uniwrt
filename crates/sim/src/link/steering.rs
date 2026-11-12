//! The ops steering sends (features/wifi.md, Steering & balancing; D-093): `btm`, as
//! `node-agent/btm.uc` has hostapd send a transition request, and `ubus` for usteer's
//! `get_client_info`, the one call the controller makes through it.

use serde_json::{Value, json, value::RawValue};
use uniwrt_core::MacAddr;
use uniwrt_node::{Result, proto::ops::code};

use super::{SimLink, bad_request, field, remote};
use crate::{fleet::lock, render, world::World};

/// hostapd's beacon interval: 100 TU.
const TBTT_US: u32 = 102_400;

impl SimLink {
    /// `btm`: the request goes from the access point `addr` is connected to; candidates name
    /// their BSSIDs in their first 6 bytes.
    pub(super) fn btm(&self, world: &mut World, op: &str, args: &RawValue) -> Result<Value> {
        let addr: MacAddr = field(op, args, "addr")?;
        let neighbors: Vec<String> = field(op, args, "neighbors")?;
        let kick_ms: u32 = field(op, args, "kick_ms")?;
        if lock(&self.shared.nodes[self.node].disk).wpad == render::wpad::BASIC {
            return Err(remote(
                op,
                code::UNSUPPORTED,
                "hostapd has no 802.11v (WNM)",
            ));
        }
        let candidates: Vec<MacAddr> = neighbors.iter().filter_map(|nr| bssid(nr)).collect();
        let band = world
            .transition(self.node, addr, &candidates)
            .map_err(|_| remote(op, code::NOT_FOUND, &format!("{addr} isn't connected")))?;
        let timer = u64::from(kick_ms) * 1000 / u64::from(TBTT_US);
        Ok(json!({"ifname": band.ap_iface(), "token": 1, "timer": timer, "mbo": false}))
    }

    /// `ubus`: usteer's `get_client_info` where usteer runs.
    pub(super) fn ubus(&self, world: &World, op: &str, args: &RawValue) -> Result<Value> {
        let path: String = field(op, args, "path")?;
        let method: String = field(op, args, "method")?;
        if path != "usteer" || method != "get_client_info" {
            return Err(bad_request(op, &format!("no {path} {method} here")));
        }
        if !lock(&self.shared.nodes[self.node].disk).steers() {
            return Err(remote(op, code::NOT_FOUND, "Not found"));
        }
        let data: Value = field(op, args, "data")?;
        let mac = data["address"]
            .as_str()
            .and_then(|a| a.parse::<MacAddr>().ok())
            .ok_or_else(|| bad_request(op, "address"))?;
        world
            .usteer_client_info(self.node, mac)
            .ok_or_else(|| remote(op, code::NOT_FOUND, "Not found"))
    }
}

/// A neighbor report's BSSID: its first 6 bytes, in hex.
fn bssid(report: &str) -> Option<MacAddr> {
    let mut octets = [0u8; 6];
    for (i, octet) in octets.iter_mut().enumerate() {
        *octet = u8::from_str_radix(report.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(MacAddr::new(octets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_candidate_names_its_bssid_first() {
        assert_eq!(
            bssid("020000000100ef0900007324090301ff"),
            Some(MacAddr::new([2, 0, 0, 0, 1, 0]))
        );
        assert_eq!(bssid("0200"), None);
        assert_eq!(bssid("zz0000000100"), None);
    }
}

//! The `leases` set: dnsmasq's lease files and odhcpd's leases over ubus. A dnsmasq line is
//! `<expiry> <mac> <ip> <hostname> <client-id>` for IPv4 and `<expiry> <iaid> <ip> <hostname>
//! <duid>` for DHCPv6, with `*` for an unknown field and expiry 0 for a lease that never ends;
//! expiry times are on the node's clock, which the set includes. A DHCPv6 client's MAC address is
//! known when its DUID is link-layer based (DUID-LLT or DUID-LL over Ethernet).

use std::{collections::BTreeMap, net::IpAddr};

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::facts::LeaseFacts;

#[derive(Deserialize)]
pub(super) struct Raw {
    now: u64,
    #[serde(default)]
    dnsmasq: Vec<LeaseFile>,
    #[serde(default)]
    odhcpd: Odhcpd,
}

#[derive(Deserialize)]
struct LeaseFile {
    data: Option<String>,
}

#[derive(Default, Deserialize)]
struct Odhcpd {
    ipv4leases: Option<Devices>,
    ipv6leases: Option<Devices>,
}

#[derive(Deserialize)]
struct Devices {
    #[serde(default)]
    device: BTreeMap<String, DeviceLeases>,
}

#[derive(Deserialize)]
struct DeviceLeases {
    #[serde(default)]
    leases: Vec<OdhcpdLease>,
}

/// ⚠️ An odhcpd DHCPv4 lease has `mac` (hex without separators) and `address`; a DHCPv6 lease
/// has `duid` and `ipv6-addr`.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct OdhcpdLease {
    mac: Option<String>,
    address: Option<String>,
    duid: Option<String>,
    hostname: Option<String>,
    valid: Option<i64>,
    #[serde(default)]
    ipv6_addr: Vec<Address>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Address {
    address: String,
    valid_lifetime: Option<i64>,
}

pub(super) fn parse(raw: Raw) -> Vec<LeaseFacts> {
    let Raw {
        now,
        dnsmasq: files,
        odhcpd: servers,
    } = raw;
    let mut leases: Vec<LeaseFacts> = files
        .iter()
        .filter_map(|file| file.data.as_deref())
        .flat_map(str::lines)
        .filter_map(|line| dnsmasq(line, now))
        .collect();
    let devices = [&servers.ipv4leases, &servers.ipv6leases];
    for (device, list) in devices.into_iter().flatten().flat_map(|d| &d.device) {
        for lease in &list.leases {
            odhcpd(device, lease, &mut leases);
        }
    }
    leases
}

fn dnsmasq(line: &str, now: u64) -> Option<LeaseFacts> {
    let [expiry, id, ip, hostname, client_id] = line.split_whitespace().collect::<Vec<_>>()[..]
    else {
        // Also the `duid <server DUID>` line before DHCPv6 leases.
        return None;
    };
    let ip: IpAddr = ip.parse().ok()?;
    let known = |field: &str| (field != "*").then(|| field.to_owned());
    let client_id = known(client_id).map(|id| hex(&id));
    let mac = if ip.is_ipv4() {
        id.parse().ok()
    } else {
        client_id.as_deref().and_then(duid_mac)
    };
    let expiry: u64 = expiry.parse().ok()?;
    Some(LeaseFacts {
        ip,
        mac,
        hostname: known(hostname),
        client_id,
        device: None,
        expires_in_s: (expiry != 0).then(|| expiry.saturating_sub(now)),
    })
}

fn odhcpd(device: &str, lease: &OdhcpdLease, leases: &mut Vec<LeaseFacts>) {
    let hostname = lease.hostname.clone().filter(|name| !name.is_empty());
    let entry = |ip, mac, client_id, valid| LeaseFacts {
        ip,
        mac,
        hostname: hostname.clone(),
        client_id,
        device: Some(device.to_owned()),
        expires_in_s: remaining(valid),
    };
    if let Some(ip) = lease.address.as_deref().and_then(|ip| ip.parse().ok()) {
        let mac = lease
            .mac
            .as_deref()
            .and_then(|mac| bytes(&hex(mac)))
            .and_then(|b| b.try_into().ok());
        leases.push(entry(ip, mac.map(MacAddr::new), None, lease.valid));
    }
    let duid = lease.duid.as_deref().map(hex);
    for address in &lease.ipv6_addr {
        if let Ok(ip) = address.address.parse() {
            let mac = duid.as_deref().and_then(duid_mac);
            leases.push(entry(
                ip,
                mac,
                duid.clone(),
                address.valid_lifetime.or(lease.valid),
            ));
        }
    }
}

/// odhcpd's remaining lifetime, where `u32::MAX` (or -1) means forever.
fn remaining(valid: Option<i64>) -> Option<u64> {
    let valid = valid?;
    (0..i64::from(u32::MAX))
        .contains(&valid)
        .then_some(valid.unsigned_abs())
}

/// Hex digits without separators, in lower case.
fn hex(text: &str) -> String {
    text.chars()
        .filter(|c| *c != ':' && *c != '-')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The MAC address in a DUID-LLT (type 1) or DUID-LL (type 3) of hardware type 1 (Ethernet).
fn duid_mac(duid: &str) -> Option<MacAddr> {
    let duid = bytes(duid)?;
    let address = match duid.get(..4)? {
        [0, 1, 0, 1] => duid.get(8..14)?,
        [0, 3, 0, 1] => duid.get(4..10)?,
        _ => return None,
    };
    Some(MacAddr::new(address.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::{duid_mac, remaining};

    #[test]
    fn link_layer_duids_carry_a_mac() {
        let mac = "02:00:00:00:04:00".parse().ok();
        assert_eq!(duid_mac("000100012d1e4a5f020000000400"), mac);
        assert_eq!(duid_mac("00030001020000000400"), mac);
        // DUID-UUID, as udhcpc/odhcp6c on OpenWrt send by default
        assert_eq!(duid_mac("0004fe76610d5eec498d9281cc3bede7dfd8"), None);
        assert_eq!(duid_mac("0001"), None);
    }

    #[test]
    fn forever_has_no_remaining_time() {
        assert_eq!(remaining(Some(3600)), Some(3600));
        assert_eq!(remaining(Some(4_294_967_295)), None);
        assert_eq!(remaining(Some(-1)), None);
        assert_eq!(remaining(None), None);
    }
}

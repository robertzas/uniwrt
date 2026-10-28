//! The `neighbors` set: from rtnetlink (`rtnl`), the agent's selection of neighbor and bridge FDB
//! entries; or else (`proc`) /proc/net/arp and each bridge's `brforward`. `brforward` is an array
//! of `struct __fdb_entry` (16 bytes: the MAC address, the port number's low byte, `is_local`,
//! the ageing timer, the port number's high byte and padding); local entries, the bridge's own
//! addresses, are left out.

use std::collections::BTreeMap;

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::facts::{FdbEntry, NeighborFacts, NeighborState};

#[derive(Deserialize)]
pub(super) struct Raw {
    rtnl: Option<Rtnl>,
    proc: Option<Proc>,
}

#[derive(Deserialize)]
struct Rtnl {
    #[serde(default)]
    neigh: Vec<Neigh>,
    #[serde(default)]
    fdb: Vec<Fdb>,
}

#[derive(Deserialize)]
struct Neigh {
    dev: String,
    dst: String,
    lladdr: String,
    #[serde(default)]
    state: u32,
    #[serde(default)]
    flags: u32,
}

#[derive(Deserialize)]
struct Fdb {
    dev: String,
    master: String,
    lladdr: String,
    vlan: Option<u16>,
}

#[derive(Deserialize)]
struct Proc {
    arp: Option<String>,
    #[serde(default)]
    bridges: BTreeMap<String, Bridge>,
}

#[derive(Deserialize)]
struct Bridge {
    brforward: String,
    /// Each port's `port_no`, such as `0x1`.
    #[serde(default)]
    ports: BTreeMap<String, String>,
}

/// The `ATF_COM` flag of an ARP entry: complete.
const ATF_COM: u32 = 0x02;
const ATF_PERM: u32 = 0x04;
/// `NTF_ROUTER`
const ROUTER: u32 = 0x80;
const FDB_ENTRY: usize = 16;

pub(super) fn parse(raw: Raw) -> (Vec<NeighborFacts>, Vec<FdbEntry>) {
    match (raw.rtnl, raw.proc) {
        (Some(rtnl), _) => (
            rtnl.neigh.iter().filter_map(neighbor).collect(),
            rtnl.fdb.iter().filter_map(fdb).collect(),
        ),
        (None, Some(proc)) => (
            proc.arp.as_deref().map(arp).unwrap_or_default(),
            proc.bridges
                .iter()
                .flat_map(|(name, bridge)| brforward(name, bridge))
                .collect(),
        ),
        (None, None) => Default::default(),
    }
}

fn neighbor(entry: &Neigh) -> Option<NeighborFacts> {
    // `NUD_*`
    let state = match entry.state {
        0x02 => Some(NeighborState::Reachable),
        0x04 => Some(NeighborState::Stale),
        0x08 => Some(NeighborState::Delay),
        0x10 => Some(NeighborState::Probe),
        0x80 => Some(NeighborState::Permanent),
        _ => None,
    };
    Some(NeighborFacts {
        ip: entry.dst.parse().ok()?,
        mac: entry.lladdr.parse().ok()?,
        device: entry.dev.clone(),
        state,
        router: entry.flags & ROUTER != 0,
    })
}

fn fdb(entry: &Fdb) -> Option<FdbEntry> {
    let mac: MacAddr = entry.lladdr.parse().ok()?;
    (!mac.is_multicast()).then(|| FdbEntry {
        mac,
        bridge: entry.master.clone(),
        port: entry.dev.clone(),
        vlan: entry.vlan,
    })
}

/// `IP address  HW type  Flags  HW address  Mask  Device` lines after a header.
fn arp(text: &str) -> Vec<NeighborFacts> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let [ip, _, flags, mac, _, device] = line.split_whitespace().collect::<Vec<_>>()[..]
            else {
                return None;
            };
            let flags = u32::from_str_radix(flags.trim_start_matches("0x"), 16).ok()?;
            (flags & ATF_COM != 0).then_some(())?;
            Some(NeighborFacts {
                ip: ip.parse().ok()?,
                mac: mac.parse().ok()?,
                device: device.to_owned(),
                state: (flags & ATF_PERM != 0).then_some(NeighborState::Permanent),
                router: false,
            })
        })
        .collect()
}

fn brforward(name: &str, bridge: &Bridge) -> Vec<FdbEntry> {
    let Ok(data) = data_encoding::BASE64.decode(bridge.brforward.as_bytes()) else {
        return Vec::new();
    };
    let ports: BTreeMap<u16, &str> = bridge
        .ports
        .iter()
        .filter_map(|(port, number)| {
            let number = u16::from_str_radix(number.trim_start_matches("0x"), 16).ok()?;
            Some((number, port.as_str()))
        })
        .collect();
    let (entries, _) = data.as_chunks::<FDB_ENTRY>();
    entries
        .iter()
        .filter_map(|entry| {
            let is_local = entry[7] != 0;
            let port = u16::from_le_bytes([entry[6], entry[12]]);
            let mac = MacAddr::new(entry[..6].try_into().ok()?);
            (!is_local && !mac.is_multicast()).then_some(())?;
            Some(FdbEntry {
                mac,
                bridge: name.to_owned(),
                port: (*ports.get(&port)?).to_owned(),
                vlan: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Bridge, arp, brforward};

    #[test]
    fn complete_arp_entries_are_neighbors() {
        let text = "IP address       HW type     Flags       HW address            Mask     Device\n\
                    192.168.1.20     0x1         0x2         3c:22:fb:12:34:56     *        br-lan\n\
                    192.168.1.21     0x1         0x0         00:00:00:00:00:00     *        br-lan\n\
                    192.168.1.1      0x1         0x6         94:83:c4:00:00:01     *        br-lan\n";
        let neighbors = arp(text);
        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0].ip.to_string(), "192.168.1.20");
        assert_eq!(neighbors[0].state, None);
        assert!(neighbors[1].state.is_some());
    }

    #[test]
    fn brforward_entries_name_their_port() {
        let entry = |mac: [u8; 6], port: u8, local: u8| {
            let mut bytes = mac.to_vec();
            bytes.extend([port, local, 0, 0, 0, 0, 0, 0, 0, 0]);
            bytes
        };
        let data = [
            entry([0x3c, 0x22, 0xfb, 0x12, 0x34, 0x56], 2, 0),
            entry([0x94, 0x83, 0xc4, 0, 0, 1], 1, 1),
            entry([0x01, 0, 0x5e, 0, 0, 1], 1, 0),
        ]
        .concat();
        let bridge = Bridge {
            brforward: data_encoding::BASE64.encode(&data),
            ports: BTreeMap::from([("lan1".into(), "0x1".into()), ("lan2".into(), "0x2".into())]),
        };
        let fdb = brforward("br-lan", &bridge);
        assert_eq!(fdb.len(), 1);
        assert_eq!(
            (fdb[0].mac.to_string().as_str(), fdb[0].port.as_str()),
            ("3c:22:fb:12:34:56", "lan2")
        );
    }
}

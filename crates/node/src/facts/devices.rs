//! The `devices` set: sysfs attributes of each device in /sys/class/net (text, as read) and the
//! counters of /proc/net/dev. A device's kind comes from its ARP hardware type and `DEVTYPE`.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;
use uniwrt_core::MacAddr;
use uniwrt_model::facts::{Counters, DeviceFacts, DeviceKind};

/// ARP hardware types (`/sys/class/net/*/type`, `linux/if_arp.h`).
mod arphrd {
    pub(super) const PPP: u32 = 512;
    pub(super) const LOOPBACK: u32 = 772;
    /// IPIP, IPv6-in-IPv6, SIT, GRE, IPv6 GRE, and none (TUN, WireGuard).
    pub(super) const TUNNELS: [u32; 6] = [768, 769, 776, 778, 823, 65534];
}

const IFF_UP: u32 = 0x1;

#[derive(Deserialize)]
pub(super) struct Raw {
    sys: BTreeMap<String, Sys>,
    #[serde(default)]
    net_dev: Option<String>,
}

/// Unreadable attributes are null: `carrier` and `speed` of a device that is down, `speed` of
/// one without a PHY.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Sys {
    #[serde(rename = "type")]
    kind: Option<String>,
    address: Option<String>,
    mtu: Option<String>,
    flags: Option<String>,
    carrier: Option<String>,
    speed: Option<String>,
    duplex: Option<String>,
    uevent: Option<String>,
    master: Option<String>,
    wireless: bool,
    physical: bool,
}

pub(super) fn parse(raw: Raw) -> Vec<DeviceFacts> {
    let mut counters = raw.net_dev.as_deref().map(net_dev).unwrap_or_default();
    raw.sys
        .into_iter()
        .map(|(name, sys)| {
            let counters = counters.remove(&name).unwrap_or_default();
            device(name, &sys, counters)
        })
        .collect()
}

fn device(name: String, sys: &Sys, counters: Counters) -> DeviceFacts {
    let kind = kind(sys);
    let ethernet = kind == DeviceKind::Ethernet;
    let flags = sys
        .flags
        .as_deref()
        .and_then(|flags| u32::from_str_radix(flags.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    DeviceFacts {
        name,
        kind,
        mac: sys
            .address
            .as_deref()
            .and_then(|mac| mac.parse::<MacAddr>().ok())
            .filter(|mac| *mac != MacAddr::default()),
        mtu: number(sys.mtu.as_deref()).unwrap_or(0),
        up: flags & IFF_UP != 0,
        carrier: sys.carrier.as_deref() == Some("1"),
        // The negotiated values of a port; bridges and VLANs report their ports'.
        speed_mbps: number(sys.speed.as_deref())
            .filter(|&speed| ethernet && speed > 0 && speed != u32::MAX),
        full_duplex: match sys.duplex.as_deref() {
            Some("full") if ethernet => Some(true),
            Some("half") if ethernet => Some(false),
            _ => None,
        },
        master: sys.master.clone(),
        counters,
    }
}

fn kind(sys: &Sys) -> DeviceKind {
    let arphrd = number(sys.kind.as_deref());
    let devtype = sys.uevent.as_deref().and_then(|uevent| {
        uevent
            .lines()
            .find_map(|line| line.strip_prefix("DEVTYPE="))
    });
    match (arphrd, devtype) {
        (Some(arphrd::LOOPBACK), _) => DeviceKind::Loopback,
        (Some(arphrd::PPP), _) | (_, Some("ppp")) => DeviceKind::Ppp,
        _ if sys.wireless => DeviceKind::Wireless,
        (_, Some("wlan")) => DeviceKind::Wireless,
        (_, Some("bridge")) => DeviceKind::Bridge,
        (_, Some("vlan")) => DeviceKind::Vlan,
        (_, Some("wireguard")) => DeviceKind::Tunnel,
        (_, Some("dsa")) => DeviceKind::Ethernet,
        (Some(arphrd), _) if arphrd::TUNNELS.contains(&arphrd) => DeviceKind::Tunnel,
        // Physical ports have a parent device; veth, dummy and macvlan devices do not.
        (Some(1), None) if sys.physical => DeviceKind::Ethernet,
        _ => DeviceKind::Other,
    }
}

/// A decimal attribute; `-1` (unknown speed) and garbage are None.
fn number(text: Option<&str>) -> Option<u32> {
    text?.trim().parse().ok()
}

/// /proc/net/dev: two header lines, then `name: 16 counters` (receive, then transmit).
fn net_dev(text: &str) -> HashMap<String, Counters> {
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (name, values) = line.split_once(':')?;
            let values: Vec<u64> = values
                .split_whitespace()
                .map(|value| value.parse().ok())
                .collect::<Option<_>>()?;
            if values.len() < 16 {
                return None;
            }
            let counters = Counters {
                rx_bytes: values[0],
                rx_packets: values[1],
                rx_errors: values[2],
                rx_dropped: values[3],
                tx_bytes: values[8],
                tx_packets: values[9],
                tx_errors: values[10],
                tx_dropped: values[11],
            };
            Some((name.trim().to_owned(), counters))
        })
        .collect()
}

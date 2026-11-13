//! The adoption port (features/adoption.md, Path C; D-105, D-106): what the controller reads on
//! the gateway (whether its segment is up, a device's MAC in its ARP table) and on a
//! factory-default device found there (its board, its identity MAC as ZTP's hello reports it).
//! Each read is a request to run and a parser of its output; nothing changes anywhere.

use std::net::Ipv4Addr;

use serde::Deserialize;
use uniwrt_core::MacAddr;

use crate::ExecRequest;

/// The segment's bridge on the gateway (render's).
pub const BRIDGE: &str = "br-adopt";

/// Whether a link that came up on the gateway's `device` is the adoption port's: the `port`
/// itself, or its bridge (whichever netifd reports).
pub fn is_port_link(device: &str, port: &str) -> bool {
    device == port || device == BRIDGE
}

/// The candidates for the device's identity, in the hello's order: `board.json`'s label MAC,
/// then the LAN bridge's, then the first port's. Each prints a line when it has one.
const IDENTITY: &str = "jsonfilter -q -i /etc/board.json -e '@.system.label_macaddr' 2>/dev/null; \
     cat /sys/class/net/br-lan/address /sys/class/net/eth0/address 2>/dev/null; exit 0";

/// netifd's status of the segment's interface (`uw_adopt`) on the gateway.
pub fn segment_status() -> ExecRequest {
    ExecRequest::new(["ubus", "call", "network.interface.uw_adopt", "status"])
}

/// Whether the segment's status says it is up with the gateway at `address`: only then is the
/// device at 192.168.1.1 one on the adoption port.
pub fn segment_up(status: &str, address: Ipv4Addr) -> bool {
    #[derive(Deserialize)]
    struct Status {
        #[serde(default)]
        up: bool,
        #[serde(rename = "ipv4-address", default)]
        ipv4: Vec<Address>,
    }
    #[derive(Deserialize)]
    struct Address {
        address: Ipv4Addr,
    }
    serde_json::from_str::<Status>(status)
        .is_ok_and(|status| status.up && status.ipv4.iter().any(|a| a.address == address))
}

/// The gateway's ARP table.
pub fn arp_table() -> ExecRequest {
    ExecRequest::new(["cat", "/proc/net/arp"])
}

/// The MAC address that `ip` resolved to on `device` in an ARP table (`/proc/net/arp`).
pub fn neighbor(arp: &str, ip: Ipv4Addr, device: &str) -> Option<MacAddr> {
    const ATF_COM: u32 = 0x2;
    arp.lines().skip(1).find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        let [address, _, flags, mac, _, dev] = fields.as_slice() else {
            return None;
        };
        let flags = u32::from_str_radix(flags.trim_start_matches("0x"), 16).ok()?;
        let resolved = flags & ATF_COM != 0;
        (resolved && *dev == device && address.parse() == Ok(ip))
            .then(|| mac.parse().ok())
            .flatten()
            .filter(|mac| unicast(*mac))
    })
}

/// The device's identity MAC candidates ([`identity_mac`] reads them).
pub fn identity() -> ExecRequest {
    ExecRequest::new(["sh", "-c", IDENTITY])
}

/// The first of [`identity`]'s candidates that is a device's address.
pub fn identity_mac(output: &str) -> Option<MacAddr> {
    output
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .find(|mac| unicast(*mac))
}

fn unicast(mac: MacAddr) -> bool {
    !mac.is_multicast() && mac.octets() != [0; 6]
}

/// `ubus call system board`.
pub fn board() -> ExecRequest {
    ExecRequest::new(["ubus", "call", "system", "board"])
}

/// What [`board`] says, as the hello reports it; what it lacks stays empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub board_name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub release: Release,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Release {
    /// Such as `24.10.4`.
    #[serde(default)]
    pub version: String,
}

impl Board {
    pub fn parse(output: &str) -> Option<Self> {
        serde_json::from_str(output).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

    #[test]
    fn the_port_s_link_is_the_port_s_or_its_bridge_s() {
        assert!(is_port_link("lan4", "lan4"));
        assert!(is_port_link("br-adopt", "lan4"));
        assert!(!is_port_link("lan3", "lan4"));
        assert!(!is_port_link("br-lan", "lan4"));
    }

    #[test]
    fn the_segment_is_up_only_at_the_gateway_s_address() {
        let up = r#"{"up":true,"ipv4-address":[{"address":"192.168.1.250","mask":24}]}"#;
        assert!(segment_up(up, Ipv4Addr::new(192, 168, 1, 250)));
        assert!(!segment_up(up, Ipv4Addr::new(192, 168, 1, 2)));
        let down = r#"{"up":false,"ipv4-address":[]}"#;
        assert!(!segment_up(down, Ipv4Addr::new(192, 168, 1, 250)));
        // ubus's answer when the interface doesn't exist.
        assert!(!segment_up("", Ipv4Addr::new(192, 168, 1, 250)));
    }

    #[test]
    fn a_neighbor_is_resolved_on_the_segment_only() {
        let arp = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         02:42:ac:11:00:03     *        br-adopt
10.0.0.5         0x1         0x2         02:42:ac:11:00:05     *        br-lan
192.168.1.1      0x1         0x0         00:00:00:00:00:00     *        br-lan
";
        let mac: MacAddr = "02:42:ac:11:00:03".parse().unwrap();
        assert_eq!(neighbor(arp, DEVICE, BRIDGE), Some(mac));
        assert_eq!(neighbor(arp, DEVICE, "br-lan"), None);
        let incomplete = arp.replace(
            "0x2         02:42:ac:11:00:03",
            "0x0         02:42:ac:11:00:03",
        );
        assert_eq!(neighbor(&incomplete, DEVICE, BRIDGE), None);
        assert_eq!(neighbor("", DEVICE, BRIDGE), None);
    }

    #[test]
    fn the_identity_is_the_first_device_address() {
        let mac: MacAddr = "94:83:c4:00:00:01".parse().unwrap();
        assert_eq!(
            identity_mac("94:83:C4:00:00:01\n02:00:00:00:00:02\n"),
            Some(mac)
        );
        assert_eq!(
            identity_mac("00:00:00:00:00:00\n94:83:c4:00:00:01\n"),
            Some(mac)
        );
        assert_eq!(identity_mac(""), None);
    }

    #[test]
    fn the_board_reads_like_the_hello() {
        let board = Board::parse(
            r#"{"kernel":"6.6.93","hostname":"OpenWrt","system":"x","model":"GL.iNet GL-MT6000",
            "board_name":"glinet,gl-mt6000","rootfs_type":"squashfs",
            "release":{"distribution":"OpenWrt","version":"24.10.4","revision":"r1"}}"#,
        )
        .unwrap();
        assert_eq!(board.board_name, "glinet,gl-mt6000");
        assert_eq!(board.model, "GL.iNet GL-MT6000");
        assert_eq!(board.hostname, "OpenWrt");
        assert_eq!(board.release.version, "24.10.4");
        assert_eq!(Board::parse("{}"), Some(Board::default()));
    }
}

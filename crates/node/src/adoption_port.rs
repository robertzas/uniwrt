//! The adoption port (features/adoption.md, Path C; D-105, D-106): what the controller reads on
//! the gateway (whether its segment is up, a device's MAC in its ARP table) and on a
//! factory-default device found there (its board, its identity MAC as ZTP's hello reports it).
//! Each read is a request to run and a parser of its output; nothing changes anywhere, but for
//! the lend ([`lend`], D-107): the gateway moves the port into a network's bridge for a moment,
//! so a device adopted on it takes its lease on the site's LAN, and gives it back by itself.

use std::{net::Ipv4Addr, time::Duration};

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

/// The gateway's lend script (`node-agent/adoption/lend.sh`).
const LEND: &str = include_str!("../../../node-agent/adoption/lend.sh");

/// Writes the lend script (stdin) where its watcher finds it again, then runs it with the
/// arguments.
const LAUNCH: &str = "mkdir -p /var/run/uniwrt-lend && \
     cat >/var/run/uniwrt-lend/lend.sh && exec sh /var/run/uniwrt-lend/lend.sh \"$@\"";

/// What the lend script answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lent {
    /// [`lend`]: the port is in the network's bridge now.
    Lent,
    /// [`lend`]: the port is lent already.
    Busy,
    /// [`keep`]: the lend lasts until the link goes down.
    Kept,
    /// [`end_lend`]: the port is back on the segment (or was never lent).
    Ended,
}

impl Lent {
    pub fn parse(output: &str) -> Option<Self> {
        match output.trim() {
            "lent" => Some(Self::Lent),
            "busy" => Some(Self::Busy),
            "kept" => Some(Self::Kept),
            "ended" => Some(Self::Ended),
            _ => None,
        }
    }
}

fn lend_command(args: &[&str]) -> ExecRequest {
    ExecRequest::new(["sh", "-c", LAUNCH, "sh"].iter().chain(args).copied()).stdin(LEND)
}

/// Lends the gateway's `port` to `interface` (the management network's netifd interface) for
/// `lasting`, rounded up to whole seconds: the gateway gives it back then unless it is
/// [`keep`]t, and once its link has been down for 10 s. Answers [`Lent::Lent`] or
/// [`Lent::Busy`].
pub fn lend(port: &str, interface: &str, lasting: Duration) -> ExecRequest {
    let seconds = lasting.as_secs() + u64::from(lasting.subsec_nanos() > 0);
    lend_command(&["lend", port, interface, &seconds.to_string()])
}

/// Makes the lend of `port` last until its link goes down ([`Lent::Kept`]).
pub fn keep(port: &str) -> ExecRequest {
    lend_command(&["keep", port])
}

/// Gives `port` back to the segment now ([`Lent::Ended`]).
pub fn end_lend(port: &str) -> ExecRequest {
    lend_command(&["end", port])
}

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
    fn the_lend_runs_the_script_from_stdin_with_its_arguments() {
        let request = lend("lan4", "lan", Duration::from_millis(90_500));
        assert_eq!(&request.argv[..2], ["sh", "-c"]);
        assert_eq!(&request.argv[3..], ["sh", "lend", "lan4", "lan", "91"]);
        assert_eq!(request.stdin.as_deref(), Some(LEND.as_bytes()));
        assert_eq!(&keep("lan4").argv[4..], ["keep", "lan4"]);
        assert_eq!(&end_lend("lan4").argv[4..], ["end", "lan4"]);
        assert_eq!(Lent::parse("lent\n"), Some(Lent::Lent));
        assert_eq!(Lent::parse("busy"), Some(Lent::Busy));
        assert_eq!(Lent::parse("kept\n"), Some(Lent::Kept));
        assert_eq!(Lent::parse("ended"), Some(Lent::Ended));
        assert_eq!(Lent::parse(""), None);
    }

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

//! What adoption checks before it changes anything (features/adoption.md, Path A): the release,
//! the resources the agent needs, and the node's identity; and the role a node seems to have.

use uniwrt_core::MacAddr;
use uniwrt_model::{
    facts::{DeviceKind, InterfaceFacts, NodeFacts},
    node::NodeRole,
};

/// The oldest release supported: the first with ucode-mod-nl80211 and dropbear with ed25519.
const MIN_RELEASE: (u32, u32) = (24, 10);
/// Memory the agent and a backup need, with room to spare (the agent's RSS is about 3 MiB).
const MIN_AVAILABLE_MEMORY: u64 = 8 << 20;
/// Room in `/tmp` (RAM) for the agent's bundle and its reader.
const MIN_TMP: u64 = 1 << 20;
/// Room on the overlay below which installing the key may fail.
const LOW_ROOT: u64 = 64 << 10;

/// Checks `/etc/openwrt_release`: OpenWrt at [`MIN_RELEASE`] or later, or a snapshot. Returns
/// the release for people, such as `OpenWrt 24.10.8`.
pub(super) fn release(openwrt_release: &str) -> Result<String, String> {
    let field = |name: &str| {
        openwrt_release.lines().find_map(|line| {
            let value = line.strip_prefix(name)?.strip_prefix('=')?;
            Some(value.trim().trim_matches(['\'', '"']).to_owned())
        })
    };
    let (Some(id), Some(version)) = (field("DISTRIB_ID"), field("DISTRIB_RELEASE")) else {
        return Err("The node does not say which OpenWrt release it runs".to_owned());
    };
    let described = format!("{id} {version}");
    if id != "OpenWrt" {
        return Err(format!(
            "The node runs {described}; only OpenWrt is supported"
        ));
    }
    if !supported(&version) {
        return Err(format!(
            "The node runs {described}; OpenWrt {}.{} or later is required",
            MIN_RELEASE.0, MIN_RELEASE.1
        ));
    }
    Ok(described)
}

/// `24.10.8`, `25.12.0-rc1` and `SNAPSHOT` pass; `23.05.5` does not.
fn supported(version: &str) -> bool {
    if version.eq_ignore_ascii_case("snapshot") {
        return true;
    }
    let mut parts = version.split(['.', '-']).map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= MIN_RELEASE,
        _ => false,
    }
}

/// Checks the resources the agent needs; returns warnings, or why the node cannot be adopted.
pub(super) fn resources(facts: &NodeFacts) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    let memory = facts.system.memory.available_bytes;
    if memory < MIN_AVAILABLE_MEMORY {
        return Err(format!(
            "Only {} of memory is available; the agent needs {}",
            mib(memory),
            mib(MIN_AVAILABLE_MEMORY)
        ));
    }
    if let Some(tmp) = &facts.system.tmp
        && tmp.available_bytes < MIN_TMP
    {
        return Err(format!(
            "Only {} is free in /tmp; the agent needs {}",
            mib(tmp.available_bytes),
            mib(MIN_TMP)
        ));
    }
    if let Some(root) = &facts.system.root
        && root.available_bytes < LOW_ROOT
    {
        warnings.push(format!(
            "The overlay is almost full ({} KiB free)",
            root.available_bytes >> 10
        ));
    }
    Ok(warnings)
}

#[expect(clippy::cast_precision_loss, reason = "for people, one decimal")]
fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20))
}

/// The node's identity: its label MAC, or else the MAC of the device behind `lan`, or else the
/// lowest globally administered MAC of its Ethernet ports.
pub(super) fn identity(facts: &NodeFacts) -> Option<MacAddr> {
    if let Some(mac) = facts.board.label_mac {
        return Some(mac);
    }
    let lan = facts
        .interfaces
        .iter()
        .find(|interface| interface.name == "lan")
        .and_then(|lan| lan.device.as_deref());
    let device_mac = |name: &str| {
        facts
            .devices
            .iter()
            .find(|device| device.name == name)
            .and_then(|device| device.mac)
    };
    lan.and_then(device_mac).or_else(|| {
        facts
            .devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Ethernet)
            .filter_map(|device| device.mac)
            .filter(|mac| !mac.is_locally_administered())
            .min()
    })
}

/// A node with an upstream on a WAN-like interface is a gateway; any other is an access point.
pub(super) fn role(facts: &NodeFacts) -> NodeRole {
    let upstream = facts.interfaces.iter().any(|interface| {
        interface.up
            && (interface.ipv4_gateway.is_some() || interface.ipv6_gateway.is_some())
            && wan_like(interface)
    });
    if upstream {
        NodeRole::Gateway
    } else {
        NodeRole::Ap
    }
}

/// Named `wan*`, or with a PPP or cellular protocol.
pub(crate) fn wan_like(interface: &InterfaceFacts) -> bool {
    interface.name.starts_with("wan")
        || matches!(
            interface.proto.as_str(),
            "pppoe" | "pppoa" | "3g" | "qmi" | "mbim" | "ncm" | "wwan"
        )
}

#[cfg(test)]
mod tests {
    use uniwrt_core::MacAddr;
    use uniwrt_model::{
        facts::{Counters, DeviceFacts, DeviceKind, InterfaceFacts, Memory, NodeFacts, Storage},
        node::NodeRole,
    };

    use super::{identity, release, resources, role};

    #[test]
    fn releases_from_24_10_and_snapshots_are_supported() {
        let file = |id: &str, version: &str| {
            format!("DISTRIB_ID='{id}'\nDISTRIB_RELEASE='{version}'\nDISTRIB_REVISION='r1'\n")
        };
        assert_eq!(
            release(&file("OpenWrt", "24.10.8")).unwrap(),
            "OpenWrt 24.10.8"
        );
        assert!(release(&file("OpenWrt", "25.12.0-rc1")).is_ok());
        assert!(release(&file("OpenWrt", "SNAPSHOT")).is_ok());
        assert!(release(&file("OpenWrt", "24.10-SNAPSHOT")).is_ok());
        let old = release(&file("OpenWrt", "23.05.5")).unwrap_err();
        assert!(old.contains("24.10 or later"), "{old}");
        assert!(release(&file("ImmortalWrt", "24.10.0")).is_err());
        assert!(release("").is_err());
    }

    fn facts(available_mib: u64, tmp_mib: u64) -> NodeFacts {
        let mut facts = NodeFacts::default();
        facts.system.memory = Memory {
            total_bytes: 256 << 20,
            available_bytes: available_mib << 20,
            ..Memory::default()
        };
        facts.system.tmp = Some(Storage {
            total_bytes: 128 << 20,
            used_bytes: 0,
            available_bytes: tmp_mib << 20,
        });
        facts.system.root = Some(Storage {
            total_bytes: 4 << 20,
            used_bytes: 4 << 20,
            available_bytes: 8 << 10,
        });
        facts
    }

    #[test]
    fn the_agent_needs_memory_and_tmp_and_a_full_overlay_is_a_warning() {
        let warnings = resources(&facts(60, 100)).unwrap();
        assert_eq!(warnings, ["The overlay is almost full (8 KiB free)"]);
        let low = resources(&facts(4, 100)).unwrap_err();
        assert!(low.starts_with("Only 4.0 MiB of memory"), "{low}");
        assert!(resources(&facts(60, 0)).is_err());
    }

    fn device(name: &str, kind: DeviceKind, mac: &str) -> DeviceFacts {
        DeviceFacts {
            name: name.to_owned(),
            kind,
            mac: Some(mac.parse().unwrap()),
            mtu: 1500,
            up: true,
            carrier: true,
            speed_mbps: None,
            full_duplex: None,
            master: None,
            counters: Counters::default(),
        }
    }

    fn interface(name: &str, proto: &str, device: &str, gateway: bool) -> InterfaceFacts {
        InterfaceFacts {
            name: name.to_owned(),
            proto: proto.to_owned(),
            up: true,
            device: Some(device.to_owned()),
            uptime_s: Some(60),
            ipv4: Vec::new(),
            ipv6: Vec::new(),
            delegated_prefixes: Vec::new(),
            ipv4_gateway: gateway.then(|| "192.0.2.1".parse().unwrap()),
            ipv6_gateway: None,
            dns_servers: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[test]
    fn identity_prefers_the_label_mac_then_lan_then_the_lowest_port() {
        let mut facts = NodeFacts {
            devices: vec![
                device("eth1", DeviceKind::Ethernet, "94:83:c4:00:00:02"),
                device("eth0", DeviceKind::Ethernet, "94:83:c4:00:00:01"),
                device("phy0-ap0", DeviceKind::Wireless, "02:00:00:00:00:01"),
                device("br-lan", DeviceKind::Bridge, "94:83:c4:00:00:03"),
            ],
            ..NodeFacts::default()
        };
        let lowest: MacAddr = "94:83:c4:00:00:01".parse().unwrap();
        assert_eq!(identity(&facts), Some(lowest));
        facts.interfaces = vec![interface("lan", "static", "br-lan", false)];
        assert_eq!(identity(&facts), "94:83:c4:00:00:03".parse().ok());
        facts.board.label_mac = "94:83:c4:00:00:09".parse().ok();
        assert_eq!(identity(&facts), facts.board.label_mac);
        assert_eq!(identity(&NodeFacts::default()), None);
    }

    #[test]
    fn a_node_with_a_wan_upstream_is_a_gateway() {
        let mut facts = NodeFacts {
            interfaces: vec![interface("lan", "dhcp", "br-lan", true)],
            ..NodeFacts::default()
        };
        assert_eq!(role(&facts), NodeRole::Ap);
        facts
            .interfaces
            .push(interface("wan", "dhcp", "eth1.201", true));
        assert_eq!(role(&facts), NodeRole::Gateway);
        facts.interfaces[1] = interface("internet", "pppoe", "pppoe-internet", true);
        assert_eq!(role(&facts), NodeRole::Gateway);
    }
}

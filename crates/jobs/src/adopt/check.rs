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

/// A release adoption refuses: what it is, for people, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unsupported {
    /// Such as `OpenWrt 23.05.5`, or `an unknown release`.
    pub(crate) release: String,
    pub(crate) why: String,
}

/// The default kernel of each OpenWrt release, newest first: a node that names no release is
/// taken for the newest whose kernel its own is at least (D-157). `web/src/lib/firmware.ts` keeps
/// the same table.
const KERNELS: [((u32, u32), (u32, u32)); 6] = [
    ((6, 12), (25, 12)),
    ((6, 6), (24, 10)),
    ((5, 15), (23, 5)),
    ((5, 10), (22, 3)),
    ((5, 4), (21, 2)),
    ((4, 14), (19, 7)),
];

/// What a node says of its release, in the order adoption trusts it: `/etc/openwrt_release`,
/// then `/usr/lib/os-release`, then the kernel (`uname -r`); each read only when the ones before
/// it name no release.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReleaseSources {
    pub(crate) openwrt_release: Option<String>,
    pub(crate) os_release: Option<String>,
    pub(crate) kernel: Option<String>,
}

impl ReleaseSources {
    /// The distribution and version the release files name, if one does.
    pub(crate) fn named(&self) -> Option<(String, String)> {
        let pair = |text: &Option<String>, id: &str, version: &str| {
            let text = text.as_deref()?;
            Some((field(text, id)?, field(text, version)?))
        };
        pair(&self.openwrt_release, "DISTRIB_ID", "DISTRIB_RELEASE")
            .or_else(|| pair(&self.os_release, "NAME", "VERSION_ID"))
    }
}

/// A `KEY='value'` or `KEY="value"` line's value; `None` when missing or empty.
fn field(text: &str, name: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            let value = line.strip_prefix(name)?.strip_prefix('=')?;
            Some(value.trim().trim_matches(['\'', '"']).to_owned())
        })
        .filter(|value| !value.is_empty())
}

/// The release branch whose default kernel `kernel` (`6.6.93`) is, as [`KERNELS`] infers it.
fn inferred(kernel: &str) -> Option<(u32, u32)> {
    let mut parts = kernel.trim().split(['.', '-']).map(str::parse::<u32>);
    let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
        return None;
    };
    KERNELS
        .iter()
        .find(|(min, _)| (major, minor) >= *min)
        .map(|&(_, release)| release)
}

/// Checks the release: OpenWrt at [`MIN_RELEASE`] or later, or a snapshot. Returns the release
/// for people, such as `OpenWrt 24.10.8`, or `OpenWrt 24.10 (inferred from Linux 6.6.93)` for
/// a node whose files name none.
pub(crate) fn release(sources: &ReleaseSources) -> Result<String, Unsupported> {
    let Some((id, version)) = sources.named() else {
        return from_kernel(sources.kernel.as_deref().map(str::trim));
    };
    let described = format!("{id} {version}");
    if id != "OpenWrt" {
        return Err(Unsupported {
            why: format!("The node runs {described}; only OpenWrt is supported"),
            release: described,
        });
    }
    if !supported(&version) {
        return Err(Unsupported {
            why: format!(
                "The node runs {described}; OpenWrt {}.{} or later is required",
                MIN_RELEASE.0, MIN_RELEASE.1
            ),
            release: described,
        });
    }
    Ok(described)
}

/// The release a node that names none runs, as its kernel tells.
fn from_kernel(kernel: Option<&str>) -> Result<String, Unsupported> {
    let unknown = || Unsupported {
        release: "an unknown release".to_owned(),
        why: "The node does not say which OpenWrt release it runs".to_owned(),
    };
    let kernel = kernel
        .filter(|kernel| !kernel.is_empty())
        .ok_or_else(unknown)?;
    let (major, minor) = inferred(kernel).ok_or_else(unknown)?;
    let described = format!("OpenWrt {major}.{minor:02} (inferred from Linux {kernel})");
    if (major, minor) < MIN_RELEASE {
        return Err(Unsupported {
            why: format!(
                "The node does not say which OpenWrt release it runs, and its kernel, Linux \
                 {kernel}, is that of {major}.{minor:02}; OpenWrt {}.{} or later is required",
                MIN_RELEASE.0, MIN_RELEASE.1
            ),
            release: described,
        });
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

/// Whether the node is the zero-touch device that called home (D-102): the same board, and
/// the MAC it gave is its identity or one of its devices'. The hello is unauthenticated, so this
/// only catches a different device at the hello's address, not one built to lie.
pub(super) fn claim(
    facts: &NodeFacts,
    identity: MacAddr,
    claim: &super::ZtpClaim,
) -> Result<(), uniwrt_model::job::JobError> {
    let mac = identity == claim.mac
        || facts
            .devices
            .iter()
            .any(|device| device.mac == Some(claim.mac));
    if mac && facts.board.board_name == claim.board_name {
        return Ok(());
    }
    Err(uniwrt_model::job::JobError {
        code: uniwrt_model::job::JobErrorCode::IdentityMismatch,
        message: format!(
            "The node at this address is a {} ({identity}), not the {} ({}) that called home",
            facts.board.board_name, claim.board_name, claim.mac
        ),
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

    use super::{ReleaseSources, identity, release, resources, role};

    fn file(id: &str, version: &str) -> ReleaseSources {
        ReleaseSources {
            openwrt_release: Some(format!(
                "DISTRIB_ID='{id}'\nDISTRIB_RELEASE='{version}'\nDISTRIB_REVISION='r1'\n"
            )),
            ..ReleaseSources::default()
        }
    }

    #[test]
    fn releases_from_24_10_and_snapshots_are_supported() {
        assert_eq!(
            release(&file("OpenWrt", "24.10.8")).unwrap(),
            "OpenWrt 24.10.8"
        );
        assert!(release(&file("OpenWrt", "25.12.0-rc1")).is_ok());
        assert!(release(&file("OpenWrt", "SNAPSHOT")).is_ok());
        assert!(release(&file("OpenWrt", "24.10-SNAPSHOT")).is_ok());
        let old = release(&file("OpenWrt", "23.05.5")).unwrap_err();
        assert_eq!(old.release, "OpenWrt 23.05.5");
        assert!(old.why.contains("24.10 or later"), "{}", old.why);
        let other = release(&file("ImmortalWrt", "24.10.0")).unwrap_err();
        assert_eq!(other.release, "ImmortalWrt 24.10.0");
        assert_eq!(
            release(&ReleaseSources::default()).unwrap_err().release,
            "an unknown release"
        );
    }

    #[test]
    fn os_release_then_the_kernel_stand_in_for_a_missing_release_file() {
        let os = ReleaseSources {
            openwrt_release: Some("DISTRIB_ID=''\n".to_owned()),
            os_release: Some("NAME=\"OpenWrt\"\nVERSION_ID=\"25.12.0\"\nID=openwrt\n".to_owned()),
            kernel: Some("6.1.0".to_owned()),
        };
        assert_eq!(release(&os).unwrap(), "OpenWrt 25.12.0");
        let kernel = |kernel: &str| ReleaseSources {
            kernel: Some(format!("{kernel}\n")),
            ..ReleaseSources::default()
        };
        assert_eq!(
            release(&kernel("6.6.93")).unwrap(),
            "OpenWrt 24.10 (inferred from Linux 6.6.93)"
        );
        assert_eq!(
            release(&kernel("6.12.40")).unwrap(),
            "OpenWrt 25.12 (inferred from Linux 6.12.40)"
        );
        // Newer than any listed: the newest branch, which a snapshot passes as.
        assert!(
            release(&kernel("6.18.2"))
                .unwrap()
                .starts_with("OpenWrt 25.12 ")
        );
        // Between two releases' kernels: the older release.
        let old = release(&kernel("6.1.120")).unwrap_err();
        assert_eq!(old.release, "OpenWrt 23.05 (inferred from Linux 6.1.120)");
        assert!(old.why.contains("24.10 or later"), "{}", old.why);
        assert_eq!(
            release(&kernel("4.9.1")).unwrap_err().release,
            "an unknown release"
        );
        assert_eq!(
            release(&kernel("junk")).unwrap_err().release,
            "an unknown release"
        );
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
    fn a_ztp_claim_needs_the_board_and_one_of_the_macs() {
        let mut facts = NodeFacts {
            devices: vec![
                device("eth0", DeviceKind::Ethernet, "94:83:c4:00:00:01"),
                device("br-lan", DeviceKind::Bridge, "94:83:c4:00:00:03"),
            ],
            ..NodeFacts::default()
        };
        facts.board.board_name = "glinet,gl-mt6000".to_owned();
        let identity = "94:83:c4:00:00:09".parse().unwrap();
        let claim = |mac: &str, board: &str| super::super::ZtpClaim {
            mac: mac.parse().unwrap(),
            board_name: board.to_owned(),
        };
        for mac in ["94:83:c4:00:00:09", "94:83:c4:00:00:03"] {
            assert_eq!(
                super::claim(&facts, identity, &claim(mac, "glinet,gl-mt6000")),
                Ok(())
            );
        }
        for (mac, board) in [
            ("94:83:c4:00:00:07", "glinet,gl-mt6000"),
            ("94:83:c4:00:00:03", "glinet,gl-mt3000"),
        ] {
            let err = super::claim(&facts, identity, &claim(mac, board)).unwrap_err();
            assert_eq!(err.code, uniwrt_model::job::JobErrorCode::IdentityMismatch);
            assert!(err.message.contains("not the"), "{}", err.message);
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

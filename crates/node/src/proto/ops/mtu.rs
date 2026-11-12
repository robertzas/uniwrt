//! `mtu`: the wired backhaul's probe (features/mesh.md, Wired backhaul; D-094), mirrored by
//! `node-agent/mtu.uc`.

use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};

use super::Op;

/// Whether the cable under `dev` carries IPv6 packets of `size` bytes: the agent pings the
/// all-nodes group on `dev` from its link-local address and answers who replied at that size.
/// A passing probe shows the peer receives `size` from this end (its reply may come back
/// fragmented), so probe a cable from both ends. The gateway's firewall lets the probe in
/// only on a backhaul's interface (render's `uw_bh` zone); APs run none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mtu {
    pub dev: String,
    pub size: u16,
}

impl Mtu {
    /// batman-adv's MTU over a cable: its header above the 1500 bytes the bridge carries.
    pub const BATMAN: u16 = 1532;
    /// The sizes the agent takes: IPv6's minimum MTU up to a jumbo frame.
    pub const MIN: u16 = 1280;
    pub const MAX: u16 = 9216;

    /// The probe of the backhaul on `port`, over the site's backhaul VLAN `vid`.
    pub fn backhaul(port: &str, vid: u16) -> Self {
        Self {
            dev: format!("{port}.{vid}"),
            size: Self::BATMAN,
        }
    }
}

impl Op for Mtu {
    const NAME: &'static str = "mtu";
    type Output = MtuProbe;
}

/// The device's MTU and the peers that answered at the probe's size (the node's own addresses
/// left out). A device whose MTU is below the size isn't pinged: it has no peers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MtuProbe {
    pub dev: String,
    pub size: u16,
    pub mtu: u32,
    pub peers: Vec<Ipv6Addr>,
}

impl MtuProbe {
    /// Whether some peer took the probe's size from this end.
    pub fn passed(&self) -> bool {
        !self.peers.is_empty() && self.mtu >= u32::from(self.size)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_backhaul_is_probed_on_its_vlan_at_batmans_mtu() {
        assert_eq!(
            serde_json::to_value(Mtu::backhaul("lan1", 4094)).unwrap(),
            json!({"dev": "lan1.4094", "size": 1532})
        );
    }

    #[test]
    fn a_probe_passes_once_a_peer_answers() {
        let answer = |mtu: u32, peers: &[&str]| -> MtuProbe {
            serde_json::from_value(
                json!({"dev": "lan1.4094", "size": 1532, "mtu": mtu, "peers": peers}),
            )
            .unwrap()
        };
        assert!(answer(1532, &["fe80::9c79:29ff:febd:2ef3"]).passed());
        assert!(!answer(1532, &[]).passed());
        assert!(!answer(1500, &[]).passed());
    }
}

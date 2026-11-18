//! A node leaving the site (features/lifecycle.md, Replace / RMA / decommission; D-131): what the
//! document says about it goes with it, so the next revision names no node that isn't adopted.

use uniwrt_core::{MacAddr, NodeId};

use super::{SiteConfig, wireless::NodeScope};

/// Drops everything `config` says about `node`: its overrides, its place in WLAN scopes, its
/// radios in the RF plan, and the DHCP reservations of `macs` (its management address). A WLAN
/// scoped to that node alone stays scoped to no node, as it was for the others. True when
/// anything changed.
pub fn remove_node(config: &mut SiteConfig, node: NodeId, macs: &[MacAddr]) -> bool {
    let mut changed = false;
    let mut keep = |kept: bool| {
        changed |= !kept;
        kept
    };
    config.nodes.retain(|entry| keep(entry.node != node));
    for wlan in &mut config.wlans {
        if let NodeScope::Only(nodes) | NodeScope::Except(nodes) = &mut wlan.nodes {
            nodes.retain(|&listed| keep(listed != node));
        }
    }
    config
        .radio
        .plan
        .radios
        .retain(|radio| keep(radio.node != node));
    for network in &mut config.networks {
        if let Some(dhcp) = network.ipv4.as_mut().and_then(|v4| v4.dhcp.as_mut()) {
            dhcp.reservations.retain(|r| keep(!macs.contains(&r.mac)));
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::{
        site::{
            network::Reservation,
            node::NodeConfig,
            wireless::{PlannedRadio, TxPower, Wlan},
        },
        wifi::{Band, ChannelWidth},
    };

    const MAC: MacAddr = MacAddr::new([0x94, 0x83, 0xc4, 0, 0, 3]);

    fn planned(node: i64) -> PlannedRadio {
        PlannedRadio {
            node: NodeId::new(node),
            radio: "radio0".to_owned(),
            band: Band::G2,
            channel: 6,
            width: ChannelWidth::Mhz20,
            tx_power: TxPower::Auto,
        }
    }

    fn site() -> SiteConfig {
        let mut config = SiteConfig::defaults(None);
        config.nodes = vec![
            NodeConfig::new(NodeId::new(2)),
            NodeConfig::new(NodeId::new(3)),
        ];
        config.radio.plan.radios = vec![planned(2), planned(3)];
        let dhcp = config.networks[0]
            .ipv4
            .as_mut()
            .and_then(|v4| v4.dhcp.as_mut())
            .unwrap();
        for (last, mac) in [(3, MAC), (40, MacAddr::new([2, 0, 0, 0, 0, 1]))] {
            dhcp.reservations.push(Reservation {
                mac,
                ip: Ipv4Addr::new(10, 0, 0, last),
                name: None,
            });
        }
        config
    }

    #[test]
    fn the_node_leaves_every_part_of_the_document() {
        let mut config = site();
        let wlan = |id: &str, scope| {
            serde_json::from_value::<Wlan>(serde_json::json!({
                "id": id, "ssid": id, "enabled": true, "network": "lan", "security": "open",
                "passphrase": null, "bands": ["2g"], "nodes": scope, "hidden": false,
                "isolation": false, "max_clients": null, "schedule": null,
            }))
            .unwrap()
        };
        config.wlans = vec![
            wlan("one", serde_json::json!({"scope": "only", "nodes": [3]})),
            wlan(
                "rest",
                serde_json::json!({"scope": "except", "nodes": [2, 3]}),
            ),
        ];

        assert!(remove_node(&mut config, NodeId::new(3), &[MAC]));
        let nodes: Vec<_> = config.nodes.iter().map(|n| n.node).collect();
        assert_eq!(nodes, [NodeId::new(2)]);
        assert_eq!(config.wlans[0].nodes, NodeScope::Only(Vec::new()));
        assert_eq!(
            config.wlans[1].nodes,
            NodeScope::Except(vec![NodeId::new(2)])
        );
        assert_eq!(config.radio.plan.radios, [planned(2)]);
        let dhcp = config.networks[0]
            .ipv4
            .as_ref()
            .unwrap()
            .dhcp
            .as_ref()
            .unwrap();
        let left: Vec<_> = dhcp.reservations.iter().map(|r| r.ip.octets()[3]).collect();
        assert_eq!(left, [40]);
    }

    #[test]
    fn a_node_the_document_never_named_changes_nothing() {
        let mut config = site();
        let before = config.clone();
        assert!(!remove_node(
            &mut config,
            NodeId::new(9),
            &[MacAddr::new([2; 6])]
        ));
        assert_eq!(config, before);
    }
}

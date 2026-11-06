//! A swconfig switch in the `network` package: one `switch_vlan` per network (each user port as
//! its profile says: by default untagged in the untagged network, tagged in the others) and on
//! the gateway one for the WAN, each tagged on its CPU port, whose `<cpu>.<vid>` devices netifd
//! then creates.

use uniwrt_model::site::node::PortConfig;
use uniwrt_uci::Section;

use crate::{plan::Plan, ports};

pub(crate) fn sections(plan: &Plan<'_>) -> Vec<Section> {
    let Some(switch) = plan.switch else {
        return Vec::new();
    };
    let Some(lan_cpu) = &switch.lan_cpu else {
        return Vec::new();
    };
    // An AP bridges its WAN ports too, through the LAN's CPU port: a second CPU port in the
    // same VLANs would loop through the bridge.
    let user: Vec<(u8, ports::Carried)> = ports::switch_ports(plan.role, switch)
        .into_iter()
        .map(|n| {
            let profile = ports::profile(plan.config, &PortConfig::switch_port(&switch.name, n));
            (n, ports::carried(profile, &plan.nets))
        })
        .collect();
    let vlan = |index: usize, vid: u16, members: Vec<String>, cpu: u8| {
        let members = [members, vec![format!("{cpu}t")]].concat();
        Section::anonymous("switch_vlan")
            .with("device", switch.name.as_str())
            .with("vlan", index.to_string())
            .with("vid", vid.to_string())
            .with("ports", members.join(" "))
    };
    let mut sections = vec![
        Section::anonymous("switch")
            .with("name", switch.name.as_str())
            .with("reset", "1")
            .with("enable_vlan", "1"),
    ];
    for (index, net) in plan.nets.iter().enumerate() {
        let members = user
            .iter()
            .filter_map(|(n, (native, tagged))| {
                if *native == Some(net.vid) {
                    Some(n.to_string())
                } else if tagged.contains(&net.vid) {
                    Some(format!("{n}t"))
                } else {
                    None
                }
            })
            .collect();
        sections.push(vlan(index + 1, net.vid, members, lan_cpu.num));
    }
    if let (Some((vid, tagged)), Some(wan_cpu)) = (
        plan.wan.as_ref().and_then(|wan| wan.switch_vlan),
        &switch.wan_cpu,
    ) {
        let members = switch
            .wan_ports
            .iter()
            .map(|p| {
                if tagged {
                    format!("{p}t")
                } else {
                    p.to_string()
                }
            })
            .collect();
        sections.push(vlan(plan.nets.len() + 1, vid, members, wan_cpu.num));
    }
    sections
}

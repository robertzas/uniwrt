//! A swconfig switch in the `network` package: one `switch_vlan` per network (its user ports
//! untagged for the untagged network, tagged for the others) and on the gateway one for the
//! WAN, each tagged on its CPU port, whose `<cpu>.<vid>` devices netifd then creates.

use uniwrt_model::node::NodeRole;
use uniwrt_uci::Section;

use crate::plan::Plan;

pub(crate) fn sections(plan: &Plan<'_>) -> Vec<Section> {
    let Some(switch) = plan.switch else {
        return Vec::new();
    };
    let Some(lan_cpu) = &switch.lan_cpu else {
        return Vec::new();
    };
    // An AP bridges its WAN ports too, through the LAN's CPU port: a second CPU port in the
    // same VLANs would loop through the bridge.
    let mut user = switch.lan_ports.clone();
    if plan.role != NodeRole::Gateway {
        user.extend(&switch.wan_ports);
    }
    let vlan = |index: usize, vid: u16, ports: &[u8], tagged: bool, cpu: u8| {
        let mut members: Vec<String> = ports
            .iter()
            .map(|p| {
                if tagged {
                    format!("{p}t")
                } else {
                    p.to_string()
                }
            })
            .collect();
        members.push(format!("{cpu}t"));
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
        sections.push(vlan(index + 1, net.vid, &user, net.tagged(), lan_cpu.num));
    }
    if let (Some((vid, tagged)), Some(wan_cpu)) = (
        plan.wan.as_ref().and_then(|wan| wan.switch_vlan),
        &switch.wan_cpu,
    ) {
        sections.push(vlan(
            plan.nets.len() + 1,
            vid,
            &switch.wan_ports,
            tagged,
            wan_cpu.num,
        ));
    }
    sections
}

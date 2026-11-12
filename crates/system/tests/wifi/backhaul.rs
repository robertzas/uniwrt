//! The wired backhaul on the lab's cable (features/mesh.md, Wired backhaul; D-094): `bh0`, a
//! veth pair between a (25.12) and b (24.10), comes over as the apply stages it. The site's
//! first render puts batman-adv on the cable's VLAN beside the networks, the port still a trunk
//! (overlay); the agent's MTU probe then passes over the VLAN from both ends, the gateway's
//! firewall letting it in through the `uw_bh` zone; [`exclusive`] renders the cable alone,
//! out of the bridge, and batman-adv and the probe still go over it.

use uniwrt_model::{facts::NodeFacts, node::NodeRole, site::SiteConfig};
use uniwrt_node::proto::ops::{Mtu, MtuProbe};
use uniwrt_render::{Backhaul, BackhaulStage, Rendered};
use uniwrt_system::{
    Node, render_with_backhaul,
    site::{AP, GATEWAY},
};

use super::wait;

/// The lab's cable, a LAN port of each node in the facts the test renders from.
pub(super) const PORT: &str = "bh0";

/// The facts a and b render from: the lab's gateway has no WAN port, so a veth stub stands in
/// for one, and the cable is a LAN port of each.
pub(super) fn facts(a: &Node, b: &Node) -> (NodeFacts, NodeFacts) {
    a.sh("ip link add eth1 type veth peer name eth1-isp", &[]);
    let mut a_facts = a.facts();
    a_facts.board.layout.wan = Some("eth1".to_owned());
    let mut b_facts = b.facts();
    for facts in [&mut a_facts, &mut b_facts] {
        facts.board.layout.lan.push(PORT.to_owned());
    }
    (a_facts, b_facts)
}

/// The site for a (the gateway) and b (the AP), the cable at `stage`.
pub(super) fn render(
    site: &SiteConfig,
    (a, a_facts): (&Node, &NodeFacts),
    (b, b_facts): (&Node, &NodeFacts),
    stage: BackhaulStage,
) -> (Rendered, Rendered) {
    let cable = [Backhaul {
        port: PORT.to_owned(),
        stage,
    }];
    (
        render_with_backhaul(site, a, GATEWAY, NodeRole::Gateway, a_facts, &cable),
        render_with_backhaul(site, b, AP, NodeRole::Ap, b_facts, &cable),
    )
}

/// The cable's VLAN device, batman-adv's hard interface on it.
fn vlan(site: &SiteConfig) -> String {
    format!("{PORT}.{}", site.mesh.wired_vlan)
}

const CONTEXT: &str = "batctl meshif bat0 n; batctl meshif bat0 if; bridge vlan show; ip -d link show bh0; logread | tail -n 30";

/// Waits until batman-adv sees the other end over the cable's VLAN.
pub(super) fn neighbor_over_cable(site: &SiteConfig, node: &Node) {
    let dev = vlan(site);
    wait(
        "a batman neighbor over the cable",
        node,
        || {
            node.sh("batctl meshif bat0 n -H", &[])
                .lines()
                .any(|line| line.contains(dev.as_str()))
        },
        CONTEXT,
    );
}

/// The MTU probe over the cable's VLAN passes from `node`'s end.
fn probe_passes(site: &SiteConfig, node: &Node) {
    let dev = vlan(site);
    let answer = node
        .agent(
            "mtu",
            &serde_json::to_value(Mtu {
                dev,
                size: Mtu::BATMAN,
            })
            .expect("a request"),
        )
        .unwrap_or_else(|err| panic!("{}: mtu: {err}", node.container));
    let probe: MtuProbe = serde_json::from_value(answer).expect("an mtu answer");
    assert!(
        probe.passed(),
        "{}: {probe:?}\n{}",
        node.container,
        node.sh(CONTEXT, &[])
    );
}

/// From the overlay the first render left: the probe passes from both ends, then the cable
/// carries batman-adv's VLAN alone, out of each bridge, and still does.
pub(super) fn exclusive(
    site: &SiteConfig,
    (a, a_facts): (&Node, &NodeFacts),
    (b, b_facts): (&Node, &NodeFacts),
) {
    for node in [a, b] {
        probe_passes(site, node);
    }
    let (gateway, ap) = render(site, (a, a_facts), (b, b_facts), BackhaulStage::Exclusive);
    a.apply(&gateway);
    b.apply(&ap);
    for node in [a, b] {
        wait(
            "the cable out of the bridge",
            node,
            || !node.bridge_vlans().contains_key(PORT),
            CONTEXT,
        );
        neighbor_over_cable(site, node);
        probe_passes(site, node);
    }
}

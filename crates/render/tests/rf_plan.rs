//! The RF plan the site follows (M6.1 b1a, D-112): render takes its channel, width and power
//! where the site leaves a radio on Auto, and the mesh's channel on every node.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use uniwrt_core::NodeId;
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        node::ManagedPackage,
        wireless::{Channel, PlannedChannel, PlannedRadio, RfPlan, TxPower},
    },
    wifi::{Band, ChannelWidth},
};
use uniwrt_render::Rendered;

use support::*;

fn planned(
    radio: &str,
    band: Band,
    channel: u16,
    width: ChannelWidth,
    power: TxPower,
) -> PlannedRadio {
    PlannedRadio {
        node: AP,
        radio: radio.to_owned(),
        band,
        channel,
        width,
        tx_power: power,
    }
}

/// The home site (its mesh on 5 GHz, every band on Auto) following a plan for the AP.
fn planned_site() -> SiteConfig {
    let mut site = home_site();
    site.radio.plan = RfPlan {
        radios: vec![
            planned(
                "radio0",
                Band::G2,
                11,
                ChannelWidth::Mhz40,
                TxPower::Dbm(10),
            ),
            planned(
                "radio1",
                Band::G5,
                149,
                ChannelWidth::Mhz40,
                TxPower::Dbm(17),
            ),
        ],
        mesh: Some(PlannedChannel {
            band: Band::G5,
            channel: 149,
            width: ChannelWidth::Mhz40,
        }),
    };
    site
}

fn render(site: &SiteConfig, node: NodeId, role: NodeRole) -> Rendered {
    run(site, node, role, &gateway_facts()).unwrap_or_else(|issues| panic!("{issues:?}"))
}

/// The radio's channel, htmode and txpower.
fn device(rendered: &Rendered, radio: &str) -> [Option<String>; 3] {
    let section = rendered.packages[&ManagedPackage::Wireless]
        .section(radio)
        .unwrap();
    ["channel", "htmode", "txpower"].map(|option| section.option(option).map(str::to_owned))
}

fn some(values: [&str; 3]) -> [Option<String>; 3] {
    values.map(|v| (!v.is_empty()).then(|| v.to_owned()))
}

#[test]
fn radios_left_on_auto_and_the_mesh_take_the_plan() {
    let site = planned_site();
    let ap = render(&site, AP, NodeRole::Ap);
    // radio1 carries the mesh: the plan's channel and width, and its power.
    assert_eq!(device(&ap, "radio1"), some(["149", "HE40", "17"]));
    // radio0's own override caps its power; the plan gives channel and width.
    assert_eq!(device(&ap, "radio0"), some(["11", "HE40", "14"]));
    assert!(
        ap.warnings.iter().all(|w| w.path != "mesh"),
        "{:?}",
        ap.warnings
    );
    // The gateway isn't in the plan yet, but its mesh takes the plan's channel and width.
    let gateway = render(&site, GATEWAY, NodeRole::Gateway);
    assert_eq!(device(&gateway, "radio1"), some(["149", "HE40", ""]));
}

#[test]
fn what_the_site_fixes_wins_over_the_plan() {
    let mut site = planned_site();
    let five = site
        .radio
        .bands
        .iter_mut()
        .find(|p| p.band == Band::G5)
        .unwrap();
    five.channel = Channel::Fixed(36);
    five.tx_power = TxPower::Dbm(20);
    let ap = render(&site, AP, NodeRole::Ap);
    // Off the plan's channel, the band's width applies (80 MHz by default).
    assert_eq!(device(&ap, "radio1"), some(["36", "HE80", "20"]));
    // A radio override's channel keeps the radio there; the plan's width followed its channel.
    let ap_config = site.nodes.iter_mut().find(|n| n.node == AP).unwrap();
    let radio0 = ap_config
        .radios
        .iter_mut()
        .find(|r| r.radio == "radio0")
        .unwrap();
    radio0.channel = Some(Channel::Fixed(1));
    radio0.tx_power = None;
    let ap = render(&site, AP, NodeRole::Ap);
    assert_eq!(device(&ap, "radio0"), some(["1", "HE20", "10"]));
}

#[test]
fn a_planned_channel_the_radio_lacks_is_left_to_the_node() {
    let mut site = planned_site();
    site.radio.plan.radios[0].channel = 13;
    let ap = render(&site, AP, NodeRole::Ap);
    assert_eq!(device(&ap, "radio0"), some(["auto", "HE20", "14"]));
    assert!(
        ap.warnings
            .iter()
            .any(|w| w.path == "node/radios/radio0" && w.message.contains("plan again")),
        "{:?}",
        ap.warnings
    );
}

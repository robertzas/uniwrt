//! The nightly decision over hand-made band plans: the 15 % threshold and movable radios, unapplied
//! changes and busy radios that would move, and the document taking a band's plan and its mesh.

use std::collections::BTreeSet;

use uniwrt_core::NodeId;
use uniwrt_model::{
    rf::BandGain,
    site::{
        SiteConfig,
        wireless::{PlannedChannel, PlannedRadio, TxPower},
    },
    wifi::{Band, ChannelWidth},
};

use super::{Nightly, decide, with_band_plan, worth};
use crate::plan::{BandPlan, Breakdown, Plan, RadioPlan, RadioRef, Reason, Search, Setting};

fn setting(channel: u16, width: ChannelWidth, dbm: Option<i16>) -> Setting {
    Setting {
        channel,
        width,
        center_frequency_mhz: 5000 + 5 * u32::from(channel),
        tx_power_dbm: dbm,
    }
}

fn radio(node: i64, now: u16, planned: u16, reasons: Vec<Reason>) -> RadioPlan {
    RadioPlan {
        node: NodeId::new(node),
        radio: "radio1".to_owned(),
        current: Some(setting(now, ChannelWidth::Mhz80, Some(20))),
        planned: setting(planned, ChannelWidth::Mhz80, Some(17)),
        changed: now != planned,
        before: Some(Breakdown::default()),
        after: Breakdown::default(),
        reasons,
    }
}

fn band(band: Band, improvement: f32, radios: Vec<RadioPlan>) -> BandPlan {
    BandPlan {
        band,
        radios,
        before: 1.0,
        after: 1.0 - improvement,
        improvement,
        search: Search::Exhaustive,
        optimal: true,
        skipped: Vec::new(),
    }
}

fn gain(band: Band, percent: u8) -> BandGain {
    BandGain { band, percent }
}

#[test]
fn a_band_is_worth_applying_from_15_percent_when_it_moves_a_radio_the_controller_may_change() {
    assert!(worth(&band(
        Band::G5,
        0.15,
        vec![radio(1, 36, 149, vec![])]
    )));
    assert!(!worth(&band(
        Band::G5,
        0.14,
        vec![radio(1, 36, 149, vec![])]
    )));
    // Better only on paper: nothing moves, or only what the controller may not change.
    assert!(!worth(&band(Band::G5, 0.4, vec![radio(1, 36, 36, vec![])])));
    assert!(!worth(&band(
        Band::G5,
        0.4,
        vec![radio(1, 36, 149, vec![Reason::Observed])]
    )));
}

#[test]
fn the_night_applies_worthy_bands_unless_changes_wait_or_a_moving_radio_is_busy() {
    let plan = Plan {
        bands: vec![
            band(Band::G2, 0.05, vec![radio(1, 1, 6, vec![])]),
            band(
                Band::G5,
                0.234,
                vec![radio(1, 36, 149, vec![]), radio(2, 52, 52, vec![])],
            ),
        ],
    };
    let none = BTreeSet::new();
    assert_eq!(
        decide(&plan, false, &none),
        Nightly::Apply(vec![gain(Band::G5, 23)])
    );
    assert_eq!(
        decide(&plan, true, &none),
        Nightly::PendingChanges(vec![gain(Band::G5, 23)])
    );
    // Node 2 stays where it is and node 1's 2.4 GHz band isn't applied: their traffic doesn't count.
    let quiet = BTreeSet::from([(NodeId::new(2), "radio1".to_owned())]);
    assert_eq!(
        decide(&plan, false, &quiet),
        Nightly::Apply(vec![gain(Band::G5, 23)])
    );
    let busy = BTreeSet::from([(NodeId::new(1), "radio1".to_owned())]);
    assert_eq!(
        decide(&plan, false, &busy),
        Nightly::Traffic(
            vec![gain(Band::G5, 23)],
            vec![RadioRef {
                node: NodeId::new(1),
                radio: "radio1".to_owned()
            }]
        )
    );
    let flat = Plan {
        bands: vec![band(Band::G5, 0.1, vec![radio(1, 36, 149, vec![])])],
    };
    assert_eq!(decide(&flat, true, &busy), Nightly::Nothing);
}

#[test]
fn the_document_takes_the_bands_movable_radios_and_the_mesh_group_channel() {
    let mut config = SiteConfig::defaults(None);
    let kept = PlannedRadio {
        node: NodeId::new(1),
        radio: "radio0".to_owned(),
        band: Band::G2,
        channel: 6,
        width: ChannelWidth::Mhz20,
        tx_power: TxPower::Auto,
    };
    config.radio.plan.radios = vec![
        kept.clone(),
        PlannedRadio {
            node: NodeId::new(3),
            radio: "radio1".to_owned(),
            band: Band::G5,
            channel: 36,
            width: ChannelWidth::Mhz80,
            tx_power: TxPower::Auto,
        },
    ];
    let mut observed = radio(3, 36, 36, vec![Reason::Observed]);
    observed.planned.tx_power_dbm = None;
    let mut mesh = radio(2, 36, 149, vec![Reason::MeshGroup { radios: 2 }]);
    mesh.planned.width = ChannelWidth::Mhz40;
    with_band_plan(
        &mut config,
        &band(
            Band::G5,
            0.3,
            vec![radio(1, 36, 100, vec![]), mesh, observed],
        ),
    );
    let planned = |node: i64, channel: u16, width: ChannelWidth| PlannedRadio {
        node: NodeId::new(node),
        radio: "radio1".to_owned(),
        band: Band::G5,
        channel,
        width,
        tx_power: TxPower::Dbm(17),
    };
    assert_eq!(
        config.radio.plan.radios,
        vec![
            kept,
            planned(1, 100, ChannelWidth::Mhz80),
            planned(2, 149, ChannelWidth::Mhz40),
        ]
    );
    assert_eq!(
        config.radio.plan.mesh,
        Some(PlannedChannel {
            band: Band::G5,
            channel: 149,
            width: ChannelWidth::Mhz40
        })
    );
    // Without a mesh radio in the band's plan, the stored mesh stays.
    with_band_plan(&mut config, &band(Band::G2, 0.3, vec![]));
    assert_eq!(config.radio.plan.mesh.map(|m| m.channel), Some(149));
}

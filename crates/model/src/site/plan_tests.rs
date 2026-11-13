//! The RF plan the site follows (schema v11, D-112): its migration and validation.

use std::collections::BTreeSet;

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    migrate::upgrade,
    validate::{Severity, validate},
    wireless::{PlannedChannel, PlannedRadio, RfPlan, TxPower},
};
use crate::wifi::{Band, ChannelWidth};

fn fixture(version: u32) -> Value {
    let text = match version {
        10 => include_str!("../../fixtures/site/v10.json"),
        _ => include_str!("../../fixtures/site/v11.json"),
    };
    serde_json::from_str(text).unwrap()
}

/// The (path, severity) of the issues of the v11 fixture after `edit`.
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = upgrade(fixture(11)).unwrap();
    edit(&mut config);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn error(path: &str) -> (String, Severity) {
    (path.to_owned(), Severity::Error)
}

fn radio(node: i64, radio: &str, band: Band, channel: u16, width: ChannelWidth) -> PlannedRadio {
    PlannedRadio {
        node: NodeId::new(node),
        radio: radio.to_owned(),
        band,
        channel,
        width,
        tx_power: TxPower::Auto,
    }
}

#[test]
fn v10_documents_follow_no_plan() {
    assert_eq!(upgrade(fixture(10)).unwrap().radio.plan, RfPlan::default());
    let plan = upgrade(fixture(11)).unwrap().radio.plan;
    assert_eq!(plan.radios.len(), 2);
    assert_eq!(
        plan.mesh,
        Some(PlannedChannel {
            band: Band::G5,
            channel: 44,
            width: ChannelWidth::Mhz80,
        })
    );
}

#[test]
fn a_plan_lists_each_radio_once_on_channels_and_widths_of_its_band() {
    let issues = issues_after(|config| {
        config.radio.plan.radios = vec![
            radio(1, "radio0", Band::G2, 6, ChannelWidth::Mhz20),
            radio(1, "radio0", Band::G2, 11, ChannelWidth::Mhz20),
            radio(2, "radio0", Band::G2, 36, ChannelWidth::Mhz20),
            radio(2, "radio1", Band::G5, 36, ChannelWidth::Mhz320),
            radio(2, "radio2", Band::G60, 2, ChannelWidth::Mhz20),
            PlannedRadio {
                tx_power: TxPower::Dbm(40),
                ..radio(2, "radio3", Band::G6, 37, ChannelWidth::Mhz320)
            },
        ];
        config.radio.plan.mesh = Some(PlannedChannel {
            band: Band::G6,
            channel: 36,
            width: ChannelWidth::Mhz160,
        });
    });
    assert_eq!(
        issues,
        vec![
            error("radio/plan/radios/1"),
            error("radio/plan/radios/2/channel"),
            error("radio/plan/radios/3/width"),
            error("radio/plan/radios/4"),
            error("radio/plan/radios/5/tx_power"),
            error("radio/plan/mesh/channel"),
        ]
    );
}

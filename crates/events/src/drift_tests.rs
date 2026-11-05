use uniwrt_core::{NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    drift::{DriftChange, DriftEnd},
    events::{Condition, EventKind},
};

use crate::{DeriveConfig, Deriver, Seed};

const N: NodeId = NodeId::new(3);

const fn at(s: i64) -> Timestamp {
    Timestamp::from_unix_ms(s * 1_000)
}

fn detected() -> DriftChange {
    DriftChange::Detected {
        node: N,
        package: "wireless".to_owned(),
        revision: RevisionId::new(7),
        removed: false,
    }
}

fn ended(how: DriftEnd) -> DriftChange {
    DriftChange::Ended {
        node: N,
        package: "wireless".to_owned(),
        how,
    }
}

#[test]
fn a_drift_raises_its_condition_once_and_its_end_clears_it() {
    let mut d = Deriver::new(DeriveConfig::default(), Seed::default());
    let raised = d.drift(&[detected()], at(10));
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].node, Some(N));
    assert_eq!(
        raised[0].condition,
        Some(Condition {
            key: "node:3/drift:wireless".to_owned(),
            raised: true
        })
    );
    assert_eq!(raised[0].what.name(), "config_drift");
    assert_eq!(d.drift(&[detected()], at(20)), []);
    let cleared = d.drift(&[ended(DriftEnd::Acknowledged)], at(70));
    assert_eq!(
        cleared[0].what,
        EventKind::ConfigDriftEnded {
            package: "wireless".to_owned(),
            how: DriftEnd::Acknowledged,
            drift_ms: Some(60_000),
        }
    );
    assert_eq!(d.drift(&[ended(DriftEnd::Reverted)], at(80)), []);
}

#[test]
fn a_drift_raised_before_a_restart_clears() {
    let seed = Seed {
        raised: vec![("node:3/drift:wireless".to_owned(), at(0))],
        ..Seed::default()
    };
    let mut d = Deriver::new(DeriveConfig::default(), seed);
    assert_eq!(d.drift(&[detected()], at(5)), []);
    let cleared = d.drift(&[ended(DriftEnd::Confirmed)], at(30));
    assert_eq!(cleared.len(), 1);
}

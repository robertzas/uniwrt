//! The simulated agent's `leds` op (D-138): turned off, the `system` set shows the LEDs dark
//! with their state held; turned on, lit again with nothing held.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use uniwrt_core::SystemClock;
use uniwrt_model::facts::{LedFacts, NodeFacts};
use uniwrt_node::{
    NodeLink as _, facts,
    proto::ops::{Collect, Leds, Set},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const WAIT: Duration = Duration::from_secs(5);

async fn leds(link: &SimLink) -> Option<LedFacts> {
    let sets = Collect {
        sets: vec![Set::System],
    };
    let mut found = NodeFacts::default();
    let _ = facts::apply(&link.request(&sets, WAIT).await.unwrap(), &mut found);
    found.system.leds
}

#[tokio::test(start_paused = true)]
async fn the_leds_go_dark_and_come_back() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let link = fleet.link(1).unwrap();
    let lit = LedFacts {
        count: 4,
        lit: 4,
        held: false,
    };
    let dark = LedFacts {
        count: 4,
        lit: 0,
        held: true,
    };
    assert_eq!(leds(&link).await, Some(lit));

    assert_eq!(link.request(&Leds { off: true }, WAIT).await.unwrap(), dark);
    assert_eq!(leds(&link).await, Some(dark));
    // Off again: still dark, nothing changes.
    assert_eq!(link.request(&Leds { off: true }, WAIT).await.unwrap(), dark);

    assert_eq!(link.request(&Leds { off: false }, WAIT).await.unwrap(), lit);
    assert_eq!(leds(&link).await, Some(lit));
}

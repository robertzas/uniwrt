//! The simulated agent's channel switch (D-113): hostapd's `switch_chan` over the `ubus` op
//! moves the access point's radio, which the `radios` set then reports.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use serde_json::{Value, json};
use uniwrt_core::SystemClock;
use uniwrt_node::{
    NodeLink as _,
    proto::ops::{Collect, Set, Ubus},
};
use uniwrt_sim::{SimConfig, SimFleet};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test(start_paused = true)]
async fn a_switch_moves_the_radio() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let ap = fleet.link(1).unwrap();
    let mut call = Ubus::new("hostapd.phy0-ap0", "switch_chan");
    let Value::Object(data) = json!({"freq": 2437, "bandwidth": 20}) else {
        unreachable!()
    };
    call.data = data;
    ap.request(&call, WAIT).await.unwrap();
    let sets = Collect {
        sets: vec![Set::Radios],
    };
    let collected = ap.request(&sets, WAIT).await.unwrap();
    let mut facts = uniwrt_model::facts::NodeFacts::default();
    uniwrt_node::facts::apply(&collected, &mut facts);
    let radio = facts
        .radios
        .iter()
        .find(|r| r.band == uniwrt_model::wifi::Band::G2)
        .unwrap();
    assert_eq!((radio.channel, radio.frequency_mhz), (Some(6), Some(2437)));

    call.path = "hostapd.phy7-ap0".to_owned();
    assert!(ap.request(&call, WAIT).await.is_err());
}

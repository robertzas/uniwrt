//! The simulated agent's channel switch (D-113): hostapd's `switch_chan` over the `ubus` op
//! moves the access point's radio, which the `radios` set then reports, with its restart
//! pending (netifd holds the old channel) until `wifi reload` restarts it.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use serde_json::{Value, json};
use uniwrt_core::SystemClock;
use uniwrt_model::{
    facts::{NodeFacts, RadioFacts},
    wifi::Band,
};
use uniwrt_node::{
    ExecRequest, NodeLink as _,
    proto::ops::{Collect, Set, Ubus},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const WAIT: Duration = Duration::from_secs(5);

async fn radio(link: &SimLink, band: Band) -> RadioFacts {
    let sets = Collect {
        sets: vec![Set::Radios],
    };
    let collected = link.request(&sets, WAIT).await.unwrap();
    let mut facts = NodeFacts::default();
    uniwrt_node::facts::apply(&collected, &mut facts);
    facts.radios.into_iter().find(|r| r.band == band).unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_switch_moves_the_radio() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let ap = fleet.link(1).unwrap();
    let mut call = Ubus::new("hostapd.phy0-ap0", "switch_chan");
    let Value::Object(data) = json!({"freq": 2462, "bandwidth": 20}) else {
        unreachable!()
    };
    call.data = data;
    assert!(!radio(&ap, Band::G2).await.restart_pending);
    ap.request(&call, WAIT).await.unwrap();
    let moved = radio(&ap, Band::G2).await;
    assert_eq!((moved.channel, moved.frequency_mhz), (Some(11), Some(2462)));
    assert!(moved.restart_pending, "netifd holds the old channel");
    assert!(!radio(&ap, Band::G5).await.restart_pending);

    ap.exec(ExecRequest::new(["wifi", "reload"])).await.unwrap();
    let restarted = radio(&ap, Band::G2).await;
    assert_eq!(restarted.channel, Some(11));
    assert!(!restarted.restart_pending);

    call.path = "hostapd.phy7-ap0".to_owned();
    assert!(ap.request(&call, WAIT).await.is_err());
}

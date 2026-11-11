//! The simulated agent's `bss` op (D-083): a BSS it turns off is down in the `radios` set (netifd
//! still lists it) and its clients are gone from `stations`; turned on, both come back; a phy the
//! node lacks is refused.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use uniwrt_core::SystemClock;
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{
    Error, NodeLink as _, facts,
    proto::ops::{Bss, Collect, Set},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const WAIT: Duration = Duration::from_secs(5);

async fn look(link: &SimLink) -> NodeFacts {
    let sets = Collect {
        sets: vec![Set::Radios, Set::Stations],
    };
    let mut found = NodeFacts::default();
    let _ = facts::apply(&link.request(&sets, WAIT).await.unwrap(), &mut found);
    found
}

/// Whether `ifname` is up, and how many stations it has.
fn ap(facts: &NodeFacts, ifname: &str) -> (bool, usize) {
    let up = facts
        .radios
        .iter()
        .flat_map(|r| &r.ifaces)
        .find(|i| i.name.as_deref() == Some(ifname))
        .unwrap_or_else(|| panic!("{ifname} is listed: {:?}", facts.radios))
        .up;
    let stations = facts.stations.iter().filter(|s| s.iface == ifname).count();
    (up, stations)
}

#[tokio::test(start_paused = true)]
async fn a_bss_goes_off_and_on_again() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let link = fleet.link(0).unwrap();
    let before = look(&link).await;
    let (up, stations) = ap(&before, "phy0-ap0");
    assert!(up && stations > 0, "the gateway's 2.4 GHz AP has clients");

    let off = Bss {
        phy: "phy0".to_owned(),
        off: vec!["phy0-ap0".to_owned()],
    };
    let set = link.request(&off, WAIT).await.unwrap();
    assert_eq!(set.radios[0].off, ["phy0-ap0"]);
    assert_eq!(ap(&look(&link).await, "phy0-ap0"), (false, 0));
    assert!(
        ap(&look(&link).await, "phy1-ap0").0,
        "the other radio's stays on"
    );

    let on = Bss {
        phy: "phy0".to_owned(),
        off: vec![],
    };
    let set = link.request(&on, WAIT).await.unwrap();
    assert_eq!(set.radios[0].on, ["phy0-ap0"]);
    let (up, stations) = ap(&look(&link).await, "phy0-ap0");
    assert!(up && stations > 0);

    let missing = Bss {
        phy: "phy7".to_owned(),
        off: vec![],
    };
    let Err(Error::Remote { code, .. }) = link.request(&missing, WAIT).await else {
        panic!("phy7 is refused");
    };
    assert_eq!(code, "not_found");
}

//! The simulated agent's `rrm` op: each access point's own neighbor report, and lists that are
//! set once and left alone when sent again.

#![allow(clippy::unwrap_used)]

use std::{collections::BTreeMap, time::Duration};

use uniwrt_core::SystemClock;
use uniwrt_node::{
    NodeLink as _,
    proto::ops::{NeighborReport, OwnReport, Rrm},
};
use uniwrt_sim::{SimConfig, SimFleet};

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test(start_paused = true)]
async fn own_reports_come_back_and_lists_are_set_once() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let (gateway, ap) = (fleet.link(0).unwrap(), fleet.link(1).unwrap());
    let own = gateway.request(&Rrm::default(), WAIT).await.unwrap().own;
    let ifnames: Vec<&str> = own.iter().map(|o| o.ifname.as_str()).collect();
    assert_eq!(ifnames, ["phy0-ap0", "phy1-ap0"]);
    assert!(
        own[1].nr.starts_with(&own[1].bssid.replace(':', "")),
        "{own:?}"
    );

    let theirs = ap.request(&Rrm::default(), WAIT).await.unwrap().own;
    let list: Vec<NeighborReport> = theirs.iter().map(OwnReport::entry).collect();
    let rrm = Rrm {
        lists: BTreeMap::from([
            ("phy1-ap0".to_owned(), list),
            ("phy9-ap0".to_owned(), Vec::new()),
        ]),
    };
    let state = gateway.request(&rrm, WAIT).await.unwrap();
    assert_eq!(state.set, ["phy1-ap0"]);
    assert_eq!(state.failed.len(), 1);
    assert_eq!(state.failed[0].ifname, "phy9-ap0");
    let again = gateway.request(&rrm, WAIT).await.unwrap();
    assert_eq!(again.set, Vec::<String>::new(), "held already");
}

//! The simulated agent's `ban` op (D-084): a banned client is kicked off the node and stays off
//! it while banned (it may go to another access point); a lift kicks nobody, and a kick alone
//! deauthenticates without keeping it off.

#![allow(clippy::unwrap_used)]

use std::time::Duration;

use uniwrt_core::{MacAddr, SystemClock};
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{
    NodeLink as _, facts,
    proto::ops::{Ban, BanEntry, Collect, Set},
};
use uniwrt_sim::{SimConfig, SimFleet, SimLink};

const WAIT: Duration = Duration::from_secs(5);

async fn stations(link: &SimLink) -> Vec<MacAddr> {
    let sets = Collect {
        sets: vec![Set::Stations],
    };
    let mut found = NodeFacts::default();
    let _ = facts::apply(&link.request(&sets, WAIT).await.unwrap(), &mut found);
    found
        .stations
        .iter()
        .filter(|s| s.iface.contains("-ap"))
        .map(|s| s.mac)
        .collect()
}

fn ban(mac: MacAddr, ms: u32, kick: bool) -> Ban {
    Ban {
        bans: vec![BanEntry {
            addr: mac.to_string(),
            ms,
            kick,
        }],
    }
}

#[tokio::test(start_paused = true)]
async fn a_banned_client_stays_off_until_lifted() {
    let fleet = SimFleet::spawn(&SimConfig::default(), &SystemClock);
    let link = fleet.link(0).unwrap();
    let mac = stations(&link).await[0];

    let banned = link.request(&ban(mac, 120_000, false), WAIT).await.unwrap();
    assert_eq!(banned.kicked.len(), 1, "{banned:?}");
    assert_eq!(banned.kicked[0].addr, mac.to_string());
    assert_eq!(banned.bsses, ["phy0-ap0", "phy1-ap0"]);
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!stations(&link).await.contains(&mac), "back while banned");
    }

    let lifted = link.request(&ban(mac, 0, false), WAIT).await.unwrap();
    assert_eq!(lifted.kicked, []);

    // A kick alone, wherever it is now: the gateway's next client.
    let other = stations(&link).await[0];
    let kicked = link.request(&ban(other, 0, true), WAIT).await.unwrap();
    assert_eq!(kicked.kicked.len(), 1, "{kicked:?}");
}

//! Scans over a real database: stored, the latest per radio, listed, read, pruned, and gone with
//! their node.

use std::time::Duration;

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    rf::{Bss, ChannelSurvey, Heard, NewScan, ScanTrigger, Security},
    wifi::{Band, ChannelWidth, WifiStandard},
};

use crate::{
    NodeRepo as _, RfRepo as _,
    repo::nodes::tests::new_node,
    testing::{START, open},
};

fn new_scan(node: NodeId, radio: &str, at: Timestamp, signal_dbm: i16) -> NewScan {
    NewScan {
        node,
        radio: radio.to_owned(),
        band: Band::G5,
        at,
        trigger: ScanTrigger::Scheduled,
        heard: Heard {
            duration_ms: 3_200,
            bsss: vec![Bss {
                bssid: MacAddr::new([2, 0, 0, 0, 0, 1]),
                ssid: Some("cafe".to_owned()),
                mesh_id: None,
                channel: 36,
                frequency_mhz: 5180,
                center_frequency_mhz: 5210,
                width: ChannelWidth::Mhz80,
                signal_dbm,
                standard: WifiStandard::He,
                security: Security::Wpa3,
                stations: Some(4),
                utilization_percent: Some(20),
            }],
            channels: vec![ChannelSurvey {
                channel: 36,
                frequency_mhz: 5180,
                noise_dbm: Some(-95),
                busy_percent: Some(12.5),
                dwell_ms: Some(110),
            }],
        },
    }
}

#[tokio::test]
async fn scans_round_trip_and_the_latest_of_each_radio_is_found() {
    let (_dir, store, _clock) = open().await;
    let node = store.create_node(new_node(1)).await.unwrap().id;
    let later = START.saturating_add(Duration::from_hours(4));
    let first = store
        .add_scan(new_scan(node, "radio1", START, -60))
        .await
        .unwrap();
    let second = store
        .add_scan(new_scan(node, "radio1", later, -62))
        .await
        .unwrap();
    let other = store
        .add_scan(new_scan(node, "radio0", START, -70))
        .await
        .unwrap();
    assert!(second.id > first.id);
    assert_eq!(store.scan(first.id).await.unwrap(), Some(first.clone()));
    assert_eq!(first.bsss[0].signal_dbm, -60);

    let latest = store.latest_scans(START).await.unwrap();
    assert_eq!(latest, [other.clone(), second.clone()]);
    let recent = store.latest_scans(later).await.unwrap();
    assert_eq!(recent, std::slice::from_ref(&second));

    let listed = store
        .scans(Some(node), Some("radio1".into()), None, 10)
        .await
        .unwrap();
    assert_eq!(listed, [second.summary(), first.summary()]);
    assert_eq!(listed[0].networks, 1);
    let page = store.scans(None, None, Some(second.id), 1).await.unwrap();
    assert_eq!(page, [first.summary()]);

    assert_eq!(store.prune_scans(later).await.unwrap(), 2);
    assert_eq!(store.scan(first.id).await.unwrap(), None);
    assert!(store.delete_node(node).await.unwrap());
    assert_eq!(
        store.scan(second.id).await.unwrap(),
        None,
        "gone with its node"
    );
}

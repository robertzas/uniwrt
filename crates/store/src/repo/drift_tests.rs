use std::time::Duration;

use uniwrt_core::{Clock as _, MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    drift::{Acknowledged, DriftRecord},
    node::{NewNode, NodeRole},
};

use super::sha256;
use crate::{DriftRepo as _, NodeRepo as _, testing};

#[test]
fn hashes_read_as_sha256sum_prints_them() {
    assert_eq!(
        sha256(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[tokio::test]
async fn drifts_and_mutes_are_kept_per_node_and_package() {
    let (_dir, store, clock) = testing::open().await;
    let node = store
        .create_node(NewNode {
            name: "ap".to_owned(),
            mac: MacAddr::new([2, 0, 0, 0, 0, 1]),
            board_name: "x86".to_owned(),
            host: "192.168.1.20".to_owned(),
            port: 22,
            role: NodeRole::Ap,
        })
        .await
        .unwrap();
    let at = clock.now();
    let mut record = DriftRecord {
        node: node.id,
        package: "wireless".to_owned(),
        sha256: Some("a".repeat(64)),
        text: Some("config wifi-device 'radio0'\n".to_owned()),
        detected_at: at,
        changed_at: at,
        acknowledged: None,
    };
    store.put_drift(record.clone()).await.unwrap();
    // A node that is gone is left out.
    store
        .put_drift(DriftRecord {
            node: NodeId::new(99),
            ..record.clone()
        })
        .await
        .unwrap();
    assert_eq!(store.drifts().await.unwrap(), [record.clone()]);
    clock.advance(Duration::from_secs(5));
    record.acknowledged = Some(Acknowledged {
        at: clock.now(),
        by: Some("admin".to_owned()),
    });
    record.sha256 = None;
    record.text = None;
    record.changed_at = Timestamp::from_unix_ms(at.as_unix_ms() + 5000);
    store.put_drift(record.clone()).await.unwrap();
    assert_eq!(store.drifts().await.unwrap(), [record]);
    assert!(
        store
            .delete_drift(node.id, "wireless".to_owned())
            .await
            .unwrap()
    );
    assert!(
        !store
            .delete_drift(node.id, "wireless".to_owned())
            .await
            .unwrap()
    );
    assert_eq!(store.drifts().await.unwrap(), []);

    let mute = |muted, by| store.set_drift_mute(node.id, "dhcp".to_owned(), muted, by);
    assert!(mute(true, Some("admin".to_owned())).await.unwrap());
    assert!(!mute(true, None).await.unwrap());
    let mutes = store.drift_mutes().await.unwrap();
    assert_eq!(mutes.len(), 1);
    assert_eq!(
        (mutes[0].package.as_str(), mutes[0].by.as_deref()),
        ("dhcp", Some("admin"))
    );
    assert!(mute(false, None).await.unwrap());
    assert!(!mute(false, None).await.unwrap());
    assert_eq!(store.drift_mutes().await.unwrap(), []);
}

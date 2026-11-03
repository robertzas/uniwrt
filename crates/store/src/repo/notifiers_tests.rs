//! Notifiers and settings over a real database: created, listed, updated, deliveries recorded,
//! deleted; a setting stored and replaced.

use std::time::Duration;

use serde_json::json;
use uniwrt_core::{Clock as _, NodeId, NotifierId};
use uniwrt_model::{
    events::EventLevel,
    notify::{NotifierConfig, NotifierFilter, NotifierSpec, NtfySpec},
};

use crate::{
    NotifierRepo as _, SettingsRepo as _,
    testing::{START, open},
};

fn config(name: &str) -> NotifierConfig {
    NotifierConfig {
        name: name.to_owned(),
        enabled: true,
        spec: NotifierSpec::Ntfy(NtfySpec {
            server: "https://ntfy.sh".to_owned(),
            topic: "home".to_owned(),
            username: None,
        }),
        filter: NotifierFilter::default(),
    }
}

#[tokio::test]
async fn notifiers_round_trip() {
    let (_dir, store, clock) = open().await;
    let phone = store.create_notifier(config("Phone")).await.unwrap();
    assert_eq!(phone.config, config("Phone"));
    assert_eq!(phone.created_at, START);
    assert_eq!(phone.status.last_ok_at, None);
    let other = store.create_notifier(config("Other")).await.unwrap();
    assert!(other.id > phone.id);

    clock.advance(Duration::from_secs(10));
    let changed = NotifierConfig {
        enabled: false,
        filter: NotifierFilter {
            min: EventLevel::Critical,
            nodes: vec![NodeId::new(3)],
            resolved: false,
            remind_s: None,
        },
        ..config("Phone 2")
    };
    let updated = store
        .update_notifier(phone.id, changed.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.config, changed);
    assert_eq!(updated.created_at, START);
    assert_eq!(updated.updated_at, clock.now());
    assert!(
        store
            .update_notifier(NotifierId::new(99), changed)
            .await
            .unwrap()
            .is_none()
    );

    store
        .record_delivery(phone.id, START, Some("HTTP 500".to_owned()))
        .await
        .unwrap();
    store
        .record_delivery(phone.id, clock.now(), None)
        .await
        .unwrap();
    let status = store.notifier(phone.id).await.unwrap().unwrap().status;
    assert_eq!(status.last_ok_at, Some(clock.now()));
    assert_eq!(status.last_error_at, Some(START));
    assert_eq!(status.last_error.as_deref(), Some("HTTP 500"));

    let all = store.notifiers().await.unwrap();
    assert_eq!(
        all.iter().map(|n| n.id).collect::<Vec<_>>(),
        [phone.id, other.id]
    );
    assert!(store.delete_notifier(phone.id).await.unwrap());
    assert!(!store.delete_notifier(phone.id).await.unwrap());
    assert!(store.notifier(phone.id).await.unwrap().is_none());
}

#[tokio::test]
async fn settings_are_replaced() {
    let (_dir, store, _clock) = open().await;
    assert_eq!(store.setting("notifications").await.unwrap(), None);
    let first = json!({ "link_url": "https://uniwrt.lan:8443" });
    store
        .put_setting("notifications", first.clone())
        .await
        .unwrap();
    assert_eq!(store.setting("notifications").await.unwrap(), Some(first));
    store.put_setting("notifications", json!({})).await.unwrap();
    assert_eq!(
        store.setting("notifications").await.unwrap(),
        Some(json!({}))
    );
}

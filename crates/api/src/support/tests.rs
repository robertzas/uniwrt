//! The support bundle over the demo: one `.tar.gz` with the controller's files and each
//! online node's facts, UCI and snapshot, the Wi-Fi status's keys masked, and its download
//! audited.

use std::time::Duration;

use axum::{
    body::to_bytes,
    http::{Method, StatusCode, header},
};
use uniwrt_model::user::AuditAction;
use uniwrt_store::AuditRepo as _;

use crate::testing::{Demo, request};

#[tokio::test]
async fn the_bundle_holds_the_controller_s_files_and_each_node_s() {
    let demo = Demo::start().await;
    // The fleet's facts first, so every node has some.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let response = demo
        .send(request(
            Method::GET,
            "/v1/support/bundle",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/gzip");
    let disposition = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        disposition.starts_with("attachment; filename=\"uniwrt-support-"),
        "{disposition}"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let entries = uniwrt_node::archive::entries(&body).unwrap();
    let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
    let root = paths[0].split('/').next().unwrap();
    assert!(root.starts_with("uniwrt-support-"), "{root}");
    let has = |path: &str| paths.contains(&format!("{root}/{path}").as_str());
    for path in [
        "README.txt",
        "controller/about.json",
        "controller/site.json",
        "controller/nodes.json",
        "controller/events.json",
        "controller/logs.txt",
        "controller/wan-monitor.json",
    ] {
        assert!(has(path), "{path} in {paths:?}");
    }
    let gateway = demo.ids[0];
    let node_dir = paths
        .iter()
        .find_map(|path| {
            let rest = path.strip_prefix(&format!("{root}/nodes/{gateway}-"))?;
            Some(format!("nodes/{gateway}-{}", rest.split('/').next()?))
        })
        .expect("the gateway's directory");
    for file in [
        "facts.json",
        "uci.json",
        "snapshot/board.json",
        "snapshot/logread.txt",
    ] {
        assert!(has(&format!("{node_dir}/{file}")), "{file} in {paths:?}");
    }
    let wireless = entries
        .iter()
        .find(|entry| entry.path == format!("{root}/{node_dir}/snapshot/wireless.json"))
        .unwrap();
    let wireless = String::from_utf8_lossy(&wireless.data);
    assert!(!wireless.contains("sim-wifi-passphrase"), "{wireless}");
    assert!(wireless.contains("********"));
    assert!(wireless.contains("uniwrt-demo"));
    let audit = demo.store.audit_log(5).await.unwrap();
    assert!(
        audit
            .iter()
            .any(|entry| entry.action == AuditAction::SupportBundle),
        "{audit:?}"
    );
}

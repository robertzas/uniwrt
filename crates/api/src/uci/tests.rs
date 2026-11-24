//! The UCI explorer's read on the demo, whose simulated nodes hold the stock `/etc/config` their
//! brownfield import read.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::Value;

use crate::testing::{Demo, json};

/// `uri` once the node is connected (409 before).
async fn ready(demo: &Demo, uri: &str) -> Value {
    for _ in 0..100 {
        let res = demo.send(demo.get(uri)).await;
        if res.status() != StatusCode::CONFLICT {
            assert_eq!(res.status(), StatusCode::OK);
            return json(res).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{uri} stayed in conflict");
}

#[tokio::test]
async fn every_package_is_read_with_its_secrets_masked() {
    let demo = Demo::start().await;
    let node = demo.ids[0].get();
    let uci = ready(&demo, &format!("/v1/nodes/{node}/uci")).await;
    assert_eq!(uci["node"], node);
    assert!(
        uci["revision"].is_number(),
        "the demo's import is its first revision"
    );
    let packages = uci["packages"].as_array().unwrap();
    let names: Vec<&str> = packages
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    for name in ["dhcp", "firewall", "network", "system", "wireless"] {
        assert!(names.contains(&name), "{names:?}");
    }
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    // The demo's nodes are observed: every package is the node's own.
    assert!(
        packages
            .iter()
            .all(|p| p["owner"] == "node" && p["error"].is_null())
    );
    let wireless = packages.iter().find(|p| p["name"] == "wireless").unwrap();
    let options: Vec<&Value> = wireless["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["options"].as_array().unwrap())
        .collect();
    let keys: Vec<&&Value> = options.iter().filter(|o| o["name"] == "key").collect();
    assert!(!keys.is_empty(), "{options:?}");
    assert!(
        keys.iter()
            .all(|o| o["secret"] == true && o["value"] == "••••••••")
    );
    let ssid = options.iter().find(|o| o["name"] == "ssid").unwrap();
    assert_eq!(ssid["secret"], false);
    assert_ne!(ssid["value"], "••••••••");
    let res = demo.send(demo.get("/v1/nodes/999/uci")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

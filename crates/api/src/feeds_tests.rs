//! The feed mirror's routes over the demo, whose upstream never answers: a node's feeds read
//! and planned, the settings saved and in force at once, a refresh refused while off, and the
//! nodes' route serving nothing it doesn't plan.

use std::time::Duration;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uniwrt_core::Timestamp;
use uniwrt_model::inventory::{Inventory, Package};
use uniwrt_store::{InventoryRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, request};

const URI: &str = "/v1/feeds";

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

async fn until(demo: &Demo, what: &str, test: impl Fn(&Value) -> bool) -> Value {
    let wait = async {
        loop {
            let (status, view) = send(demo, Method::GET, URI, None).await;
            assert_eq!(status, StatusCode::OK);
            if test(&view) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap_or_else(|_| panic!("{what} within 10 s"))
}

async fn fetch(demo: &Demo, path: &str) -> StatusCode {
    let request = Request::get(format!("/feeds/{path}"))
        .body(Body::empty())
        .unwrap();
    crate::feed_files(demo.api.clone())
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn the_mirror_plans_the_nodes_feeds_and_follows_its_settings() {
    let demo = Demo::start_with(false).await;
    let node = demo.store.nodes().await.unwrap()[0].id;
    let inventory = Inventory {
        taken_at: Timestamp::from_unix_ms(1_000),
        packages: vec![Package {
            name: "luci".to_owned(),
            version: "1".to_owned(),
            explicit: true,
        }],
        feeds: Vec::new(),
    };
    demo.store.set_inventory(node, inventory).await.unwrap();
    let view = until(&demo, "the node's feeds read and planned", |v| {
        v["sets"][0]["feeds"][0]["error"].is_string() && v["sync_error"].is_string()
    })
    .await;
    assert_eq!(
        view["settings"],
        json!({"enabled": true, "whole_feeds": false})
    );
    let feed = &view["sets"][0]["feeds"][0];
    assert!(
        feed["url"]
            .as_str()
            .unwrap()
            .starts_with("https://downloads.openwrt.org/releases/"),
        "{view}"
    );
    assert!(
        feed["error"].as_str().unwrap().contains("offline"),
        "{view}"
    );
    assert!(view["sync_error"].is_string());

    // Nothing kept yet: the index can't be had now, and nothing else is served.
    let url = feed["url"].as_str().unwrap();
    let path = url.strip_prefix("https://").unwrap();
    let apk = view["sets"][0]["set"]["manager"] == "apk";
    let index = if apk { "packages.adb" } else { "Packages.gz" };
    assert_eq!(
        fetch(&demo, &format!("{path}/{index}")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fetch(&demo, &format!("{path}/luci_1_all.ipk")).await,
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        fetch(&demo, "example.org/x/Packages.gz").await,
        StatusCode::NOT_FOUND
    );

    let refresh = format!("{URI}/refresh");
    let (status, _) = send(&demo, Method::POST, &refresh, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let off = json!({"enabled": false, "whole_feeds": true});
    let (status, view) = send(&demo, Method::PUT, &format!("{URI}/settings"), Some(&off)).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["settings"], off, "in force at once");
    let (status, _) = send(&demo, Method::POST, &refresh, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        fetch(&demo, &format!("{path}/{index}")).await,
        StatusCode::NOT_FOUND
    );
}

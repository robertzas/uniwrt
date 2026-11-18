//! DNS filtering's routes over the demo: the catalog and the gateway left alone while nothing
//! manages it, a refresh refused while blocking is off and started once a saved revision turns
//! it on, its lists then downloaded.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::Value;
use uniwrt_model::site::{
    blocking::CATALOG,
    revision::{NewRevision, RevisionKind},
};
use uniwrt_store::RevisionRepo as _;

use crate::testing::{Demo, json, request};

const URI: &str = "/v1/services/blocklists";

async fn send(demo: &Demo, method: Method, uri: &str) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, None, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

/// The view, once `test` holds.
async fn until(demo: &Demo, what: &str, test: impl Fn(&Value) -> bool) -> Value {
    let wait = async {
        loop {
            let (status, view) = send(demo, Method::GET, URI).await;
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

#[tokio::test]
async fn a_refresh_waits_for_blocking_then_downloads_the_lists() {
    let demo = Demo::start_with(false).await;
    let view = until(&demo, "the gateway", |v| v["sync"]["state"] == "unmanaged").await;
    assert_eq!(view["catalog"].as_array().unwrap().len(), CATALOG.len());
    assert!(view["gateway"].is_number(), "{view}");
    assert_eq!(view["enabled"], false);
    assert_eq!(view["lists"], Value::Array(Vec::new()));

    let refresh = format!("{URI}/refresh");
    let (status, _) = send(&demo, Method::POST, &refresh).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let head = demo.store.head_revision().await.unwrap().unwrap();
    let mut config = head.config;
    config.services.blocking.enabled = true;
    config.services.blocking.lists = vec!["urlhaus".to_owned()];
    let revision = NewRevision {
        base: Some(head.meta.id),
        kind: RevisionKind::Edit,
        restored_from: None,
        author_id: None,
        author: None,
        message: "blocking".to_owned(),
        config,
    };
    demo.store
        .commit_revision(revision, Vec::new())
        .await
        .unwrap();
    // The saved revision's lists are downloaded before any apply.
    let view = until(&demo, "the list", |v| v["lists"][0]["domains"] == 2).await;
    assert_eq!(view["lists"][0]["id"], "urlhaus");
    assert_eq!(view["enabled"], false, "nothing applied to the gateway");
    let (status, view) = send(&demo, Method::POST, &refresh).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{view}");
    assert_eq!(view["lists"].as_array().unwrap().len(), 1);
}

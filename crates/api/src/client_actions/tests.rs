//! The client actions on the demo, its gateway managing `wireless`: a kick reaches it and is
//! audited, a block and an unblock show on the client and are audited, and bad requests are
//! refused.

use std::{collections::BTreeSet, time::Duration};

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::time::Instant;
use uniwrt_model::site::{
    node::{ManagedPackage, NodeConfig},
    revision::{NewRevision, RevisionKind},
};
use uniwrt_store::{AuditRepo as _, RevisionRepo as _};

use crate::testing::{Demo, json, post, request};

/// A Wi-Fi client online at the gateway.
async fn gateway_client(demo: &Demo) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let gateway = demo.ids[0].get();
    loop {
        let clients = json(demo.send(demo.get("/v1/clients")).await).await;
        let found = clients.as_array().unwrap().iter().find(|c| {
            c["online"] == true
                && c["connection"]["medium"] == "wifi"
                && c["connection"]["node"] == gateway
        });
        if let Some(client) = found {
            return client["mac"].as_str().unwrap().to_owned();
        }
        assert!(Instant::now() < deadline, "no Wi-Fi client at the gateway");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Has the gateway manage `wireless` in a new revision.
async fn manage_gateway(demo: &Demo) {
    let head = demo.store.head_revision().await.unwrap().unwrap();
    let mut config = head.config;
    let mut node = NodeConfig::new(demo.ids[0]);
    node.managed = BTreeSet::from([ManagedPackage::Wireless]);
    config.nodes.push(node);
    let revision = NewRevision {
        base: Some(head.meta.id),
        kind: RevisionKind::Edit,
        restored_from: None,
        author_id: None,
        author: None,
        message: "manage the gateway".to_owned(),
        config,
    };
    demo.store
        .commit_revision(revision, Vec::new())
        .await
        .unwrap();
}

async fn last_audit(demo: &Demo) -> Option<String> {
    demo.store.audit_log(1).await.unwrap()[0].detail.clone()
}

#[tokio::test]
async fn kicks_and_blocks_reach_the_managed_gateway_and_are_audited() {
    let demo = Demo::start().await;
    let mac = gateway_client(&demo).await;
    manage_gateway(&demo).await;

    let kick = |body: Value| {
        post(
            &format!("/v1/clients/{mac}/kick"),
            &body,
            Some(&demo.cookie),
        )
    };
    let res = demo.send(kick(json!({ "ban_secs": 60 }))).await;
    assert_eq!(res.status(), StatusCode::OK);
    let outcome = json(res).await;
    assert_eq!(outcome["nodes"], 1, "{outcome}");
    assert_eq!(outcome["failed"], json!([]), "{outcome}");
    assert_eq!(
        last_audit(&demo).await.as_deref(),
        Some(&*format!("{mac}, kept off for 60 s"))
    );
    let res = demo.send(kick(json!({ "ban_secs": 86_401 }))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = demo.send(kick(json!({ "ban": 1 }))).await;
    assert!(res.status().is_client_error());

    let block = |method| {
        request(
            method,
            &format!("/v1/clients/{mac}/block"),
            None,
            Some(&demo.cookie),
        )
    };
    let res = demo.send(block(Method::PUT)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(json(res).await["blocked"].is_number());
    assert_eq!(
        last_audit(&demo).await.as_deref(),
        Some(&*format!("{mac} blocked"))
    );
    let res = demo.send(block(Method::DELETE)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["blocked"], Value::Null);
    assert_eq!(
        last_audit(&demo).await.as_deref(),
        Some(&*format!("{mac} unblocked"))
    );

    let res = demo
        .send(request(
            Method::PUT,
            "/v1/clients/02:00:00:00:00:fe/block",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::JobId;
use uniwrt_model::apply::{Applied, AppliedNode};
use uniwrt_store::{ApplyRepo as _, RevisionRepo as _};

use crate::testing::{Demo, json, request};

/// What the controller confirmed on the gateway, which its `system` file isn't.
const CONFIRMED: &str = "\nconfig system\n\toption hostname 'elsewhere'\n";

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> StatusCode {
    demo.send(request(method, uri, body, Some(&demo.cookie)))
        .await
        .status()
}

async fn drift(demo: &Demo) -> Value {
    let res = demo.send(demo.get("/v1/drift")).await;
    assert_eq!(res.status(), StatusCode::OK);
    json(res).await
}

async fn until(demo: &Demo, want: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..200 {
        let state = drift(demo).await;
        if want(&state) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("drift never got there: {}", drift(demo).await);
}

#[tokio::test]
async fn a_drift_is_shown_acknowledged_and_muted() {
    let demo = Demo::start().await;
    let gateway = demo.ids[0];
    let head = demo.store.head_revision().await.unwrap().unwrap();
    demo.store
        .record_applied(Applied {
            revision: head.meta.id,
            job: JobId::new(1),
            nodes: vec![AppliedNode {
                node: gateway,
                host: None,
                packages: [("system".to_owned(), CONFIRMED.to_owned())].into(),
            }],
        })
        .await
        .unwrap();
    let state = until(&demo, |s| !s["drifts"].as_array().unwrap().is_empty()).await;
    let found = &state["drifts"][0];
    assert_eq!(
        (&found["node"], &found["package"]),
        (&json!(gateway.get()), &json!("system"))
    );
    assert_eq!(found["removed"], false);
    assert_ne!(found["diff"]["sections"], json!([]));

    let base = format!("/v1/nodes/{gateway}/drift");
    let ack = format!("{base}/system/acknowledge");
    assert_eq!(
        send(&demo, Method::POST, &ack, None).await,
        StatusCode::NO_CONTENT
    );
    let state = drift(&demo).await;
    assert_eq!(state["drifts"][0]["acknowledged"]["by"], "admin");
    assert_eq!(
        send(
            &demo,
            Method::POST,
            &format!("{base}/dhcp/acknowledge"),
            None
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &demo,
            Method::POST,
            &format!("{base}/a.b/acknowledge"),
            None
        )
        .await,
        StatusCode::BAD_REQUEST
    );

    let mute = format!("{base}/system/mute");
    let muted = json!({"muted": true});
    assert_eq!(
        send(&demo, Method::PUT, &mute, Some(&muted)).await,
        StatusCode::NO_CONTENT
    );
    let state = drift(&demo).await;
    assert_eq!(state["drifts"], json!([]));
    assert_eq!(state["muted"][0]["package"], "system");
    let scoped = json!({"drift": {"node": gateway.get(), "package": "system"}});
    assert_eq!(
        send(&demo, Method::POST, "/v1/import/preview", Some(&scoped)).await,
        StatusCode::NOT_FOUND,
        "a muted package has no drift to import"
    );

    let watched = json!({"muted": false});
    assert_eq!(
        send(&demo, Method::PUT, &mute, Some(&watched)).await,
        StatusCode::NO_CONTENT
    );
    until(&demo, |s| !s["drifts"].as_array().unwrap().is_empty()).await;
    // The test's nodes have no pinned host key: the drift's node can't be read.
    assert_eq!(
        send(&demo, Method::POST, "/v1/import/preview", Some(&scoped)).await,
        StatusCode::BAD_GATEWAY
    );
}

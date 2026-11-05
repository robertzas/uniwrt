use axum::http::{Method, StatusCode};
use serde_json::json;
use uniwrt_model::site::revision::{NewRevision, RevisionKind};
use uniwrt_store::RevisionRepo as _;

use crate::testing::{Demo, json, request};

async fn preview(demo: &Demo) -> serde_json::Value {
    let res = demo
        .send(request(
            Method::POST,
            "/v1/import/preview",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    json(res).await
}

async fn accept(demo: &Demo, id: &serde_json::Value) -> StatusCode {
    let body = json!({"id": id, "message": "From the routers"});
    demo.send(request(
        Method::POST,
        "/v1/import/accept",
        Some(&body),
        Some(&demo.cookie),
    ))
    .await
    .status()
}

#[tokio::test]
async fn a_preview_is_held_until_accepted_once() {
    let demo = Demo::start().await;
    let head = demo.store.head_revision().await.unwrap().unwrap();
    let preview = preview(&demo).await;
    assert_eq!(preview["base"], head.meta.id.get());
    let nodes = preview["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), demo.ids.len());
    assert_eq!(nodes[0]["role"], "gateway", "the gateway first");
    assert!(
        nodes
            .iter()
            .all(|n| n["state"] == "unread" && n["reason"].as_str().unwrap().contains("host key")),
        "the test's nodes have no pinned host key: {nodes:?}"
    );
    assert_eq!(preview["changes"], json!([]));
    assert_eq!(accept(&demo, &json!("nope")).await, StatusCode::NOT_FOUND);
    assert_eq!(accept(&demo, &preview["id"]).await, StatusCode::OK);
    let after = demo.store.head_revision().await.unwrap().unwrap();
    assert_eq!(
        after.meta.id, head.meta.id,
        "nothing to save, nothing saved"
    );
    assert_eq!(
        accept(&demo, &preview["id"]).await,
        StatusCode::NOT_FOUND,
        "accepted once"
    );
}

#[tokio::test]
async fn a_preview_of_an_older_revision_is_refused() {
    let demo = Demo::start().await;
    let preview = preview(&demo).await;
    let head = demo.store.head_revision().await.unwrap().unwrap();
    let mut config = head.config.clone();
    config.site.name = "Elsewhere".to_owned();
    demo.store
        .commit_revision(
            NewRevision {
                base: Some(head.meta.id),
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: None,
                author: None,
                message: String::new(),
                config,
            },
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(accept(&demo, &preview["id"]).await, StatusCode::CONFLICT);
}

#[tokio::test]
async fn controllers_without_a_dial_cannot_import() {
    let demo = Demo::start_with(false).await;
    let res = demo
        .send(request(
            Method::POST,
            "/v1/import/preview",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = demo
        .send(request(
            Method::POST,
            "/v1/import/accept",
            Some(&json!({"id": "x", "extra": 1})),
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = demo
        .send(request(Method::POST, "/v1/import/preview", None, None))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

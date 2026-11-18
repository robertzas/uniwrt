//! The node backups' routes (D-126) over the demo fleet.

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use uniwrt_model::{inventory::BackupReason, node::NodeMode, user::AuditAction};
use uniwrt_node::archive;
use uniwrt_store::{AuditRepo as _, BackupRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let res = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = res.status();
    (status, json(res).await)
}

#[tokio::test]
async fn an_admin_backs_a_node_up_downloads_and_restores_it() {
    let demo = Demo::start().await;
    let id = demo.ids[1].get();
    let uri = format!("/v1/nodes/{id}/backups");

    let (status, job) = send(&demo, Method::POST, &uri, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!((&job["kind"], &job["node"]), (&json!("backup"), &json!(id)));
    assert_eq!(job["nodes"][0]["state"], "pending");
    // The test's dial can't reach the simulated nodes (the jobs' tests take backups): keep one.
    let tarball = archive::pack(&[("etc/config/network", b"config interface 'lan'\n")], 0);
    let sealed = demo
        .api
        .vault()
        .unwrap()
        .seal_backup(
            BackupReason::Manual,
            demo.api.now(),
            &tarball,
            archive::content_digest(&tarball).ok(),
        )
        .unwrap();
    demo.store.keep_backup(demo.ids[1], sealed).await.unwrap();

    let (status, backups) = send(&demo, Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let newest = &backups[0];
    assert_eq!(newest["reason"], "manual");
    assert_eq!(newest["taken_at"], newest["seen_at"]);
    let backup = newest["id"].as_i64().unwrap();

    let res = demo
        .send(request(
            Method::GET,
            &format!("{uri}/{backup}/archive"),
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/gzip");
    let disposition = res.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        disposition.starts_with("attachment; filename=\"ap-") && disposition.ends_with(".tar.gz\""),
        "{disposition}"
    );
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), tarball.as_slice());
    let audit = demo.store.audit_log(1).await.unwrap();
    assert_eq!(audit[0].action, AuditAction::Backup);
    assert!(
        audit[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("downloaded")
    );

    // Restoring needs management.
    let restore = format!("{uri}/{backup}/restore");
    let (status, _) = send(&demo, Method::POST, &restore, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "observe mode");
    demo.store
        .set_node_mode(demo.ids[1], NodeMode::Managed)
        .await
        .unwrap();
    let (status, job) = send(&demo, Method::POST, &restore, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["kind"], "restore");
    let steps: Vec<_> = job["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        steps,
        [
            "connect", "check", "backup", "stage", "apply", "verify", "confirm"
        ]
    );
    let audit = demo.store.audit_log(1).await.unwrap();
    assert!(audit[0].detail.as_deref().unwrap().starts_with("restored"));
    // Another node's backup isn't this one's.
    let other = format!("/v1/nodes/{}/backups/{backup}/archive", demo.ids[0]);
    let (status, _) = send(&demo, Method::GET, &other, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_nodes_backups_and_strangers_are_refused() {
    let demo = Demo::start().await;
    let (status, _) = send(&demo, Method::GET, "/v1/nodes/999/backups", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&demo, Method::GET, "/v1/nodes/x/backups", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let id = demo.ids[0];
    let (status, _) = send(
        &demo,
        Method::GET,
        &format!("/v1/nodes/{id}/backups/999/archive"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &demo,
        Method::POST,
        &format!("/v1/nodes/{id}/backups/999/restore"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let res = demo
        .send(request(
            Method::GET,
            &format!("/v1/nodes/{id}/backups"),
            None,
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

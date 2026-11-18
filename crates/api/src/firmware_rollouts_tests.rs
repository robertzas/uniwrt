//! Rolling upgrades through the router (D-125): the gates, a rollout started and audited, its
//! node's job failing (the sim's addresses don't answer SSH) and halting it, then skipped and
//! resumed to its end. The driver itself is tested in `uniwrt-jobs`.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::NodeId;
use uniwrt_firmware::testing::sysupgrade_image;
use uniwrt_model::{node::NodeMode, user::AuditAction};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::{
    firmware_tests::{settled, upload},
    testing::{Demo, json as body, post, request},
};

async fn send(demo: &Demo, uri: &str, what: &Value) -> (StatusCode, Value) {
    let res = demo.send(post(uri, what, Some(&demo.cookie))).await;
    (res.status(), body(res).await)
}

/// The rollout once its state is `state`.
async fn reaches(demo: &Demo, id: i64, state: &str) -> Value {
    let uri = format!("/v1/firmware/rollouts/{id}");
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let res = demo
                .send(request(Method::GET, &uri, None, Some(&demo.cookie)))
                .await;
            let rollout = body(res).await;
            if rollout["state"] == state {
                return rollout;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("rollout {id} gets {state}"))
}

/// The demo with its first two devices, an upload that fits the first, and the rollout of it.
async fn prepared() -> (Demo, i64, i64, Value, Value) {
    let demo = Demo::start_firmware(true, None, None).await;
    let nodes = settled(&demo).await["nodes"].as_array().unwrap().clone();
    let board = nodes[0]["board"].as_str().unwrap().to_owned();
    let id = nodes[0]["node"].as_i64().unwrap();
    let other = nodes[1]["node"].as_i64().unwrap();
    let res = demo
        .send(upload(
            &demo,
            "custom.bin",
            sysupgrade_image(&board, "25.12.7", 20_000),
        ))
        .await;
    let sha256 = body(res).await["sha256"].as_str().unwrap().to_owned();
    let choice = json!({"source": "upload", "sha256": sha256});
    (demo, id, other, choice, nodes[0]["version"].clone())
}

async fn manage(demo: &Demo, nodes: [i64; 2]) {
    for node in nodes {
        demo.store
            .set_node_mode(NodeId::new(node), NodeMode::Managed)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_rollout_needs_managed_devices_each_once_and_a_sensible_canary_and_window() {
    let (demo, id, other, choice, _) = prepared().await;
    let start = json!({"nodes": [{"node": id, "choice": choice}]});
    let (status, answer) = send(&demo, "/v1/firmware/rollouts", &start).await;
    assert_eq!(status, StatusCode::CONFLICT, "observe mode: {answer}");
    assert!(answer["detail"].as_str().unwrap().contains("observe mode"));
    manage(&demo, [id, other]).await;
    for refused in [
        json!({"nodes": []}),
        json!({"nodes": [{"node": id, "choice": choice}, {"node": id, "choice": choice}]}),
        json!({"nodes": [{"node": id, "choice": choice}, {"node": other, "choice": choice}],
               "canary": {"soak_min": 5}}),
        json!({"nodes": [{"node": id, "choice": choice}],
               "window": {"start_min": 600, "end_min": 610}}),
    ] {
        let (status, answer) = send(&demo, "/v1/firmware/rollouts", &refused).await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}: {answer}");
    }
    let (status, _) = send(
        &demo,
        "/v1/firmware/rollouts",
        &json!({"nodes": [{"node": id, "choice": {"source": "upload", "url": "https://evil"}}]}),
    )
    .await;
    assert!(status.is_client_error(), "no URL from the client: {status}");
}

#[tokio::test]
async fn a_rollout_runs_each_node_by_a_job_and_halts_on_a_failure() {
    let (demo, id, other, choice, version) = prepared().await;
    manage(&demo, [id, other]).await;
    let start = json!({"nodes": [{"node": id, "choice": choice}]});
    let (status, rollout) = send(&demo, "/v1/firmware/rollouts", &start).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{rollout}");
    let rid = rollout["id"].as_i64().unwrap();
    assert_eq!(rollout["nodes"][0]["from"], version);
    let audit = demo.store.audit_log(1).await.unwrap();
    assert_eq!(audit[0].action, AuditAction::Firmware);
    assert!(
        audit[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with(&format!("started rollout {rid} upgrading ")),
        "{:?}",
        audit[0].detail
    );

    // The image resolved, its job started and failed at the login: halted on it.
    let halted = reaches(&demo, rid, "halted").await;
    let node = &halted["nodes"][0];
    assert_eq!(node["state"], "failed", "{halted}");
    assert!(node["job"].is_i64(), "{halted}");
    assert!(
        halted["error"]
            .as_str()
            .unwrap()
            .contains("'s upgrade failed: "),
        "{halted}"
    );
    let (status, _) = send(&demo, "/v1/firmware/rollouts", &start).await;
    assert_eq!(status, StatusCode::CONFLICT, "one rollout at a time");
    let (status, _) = send(
        &demo,
        &format!("/v1/firmware/rollouts/{rid}/pause"),
        &json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a halted rollout isn't paused"
    );

    let (status, skipped) = send(
        &demo,
        &format!("/v1/firmware/rollouts/{rid}/nodes/{id}/skip"),
        &json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{skipped}");
    assert_eq!(skipped["nodes"][0]["state"], "skipped");
    let (status, _) = send(
        &demo,
        &format!("/v1/firmware/rollouts/{rid}/resume"),
        &json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    reaches(&demo, rid, "done").await;
    let res = demo
        .send(request(
            Method::GET,
            "/v1/firmware/rollouts",
            None,
            Some(&demo.cookie),
        ))
        .await;
    let listed = body(res).await;
    assert_eq!(listed[0]["id"], rid);
    let (status, _) = send(&demo, "/v1/firmware/rollouts/999/cancel", &json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

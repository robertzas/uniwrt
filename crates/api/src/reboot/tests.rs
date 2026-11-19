//! `POST v1/reboots` on the demo: managed nodes only, a job started and audited.

use axum::http::{StatusCode, header};
use serde_json::json;
use uniwrt_model::node::NodeMode;
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, post};

#[tokio::test]
async fn managed_nodes_restart_in_a_job() {
    let demo = Demo::start().await;
    let reboot = |nodes: serde_json::Value| {
        post(
            "/v1/reboots",
            &json!({ "nodes": nodes }),
            Some(&demo.cookie),
        )
    };
    let [gateway, ap, ..] = demo.ids[..] else {
        panic!("two nodes at least");
    };

    let res = demo.send(reboot(json!([]))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = demo.send(reboot(json!([ap.get()]))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let problem = json(res).await;
    assert!(
        problem["detail"].as_str().unwrap().contains("observe mode"),
        "{problem}"
    );

    for id in [gateway, ap] {
        demo.store
            .set_node_mode(id, NodeMode::Managed)
            .await
            .unwrap();
    }
    let res = demo.send(reboot(json!([gateway.get(), ap.get()]))).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let job = json(res).await;
    assert_eq!(location, format!("/api/v1/jobs/{}", job["id"]));
    assert_eq!(job["kind"], "reboot");
    let order: Vec<_> = job["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["node"].clone())
        .collect();
    assert_eq!(
        order,
        [json!(ap.get()), json!(gateway.get())],
        "the gateway last"
    );
    let audited = demo.store.audit_log(1).await.unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert!(audited.starts_with("restarted "), "{audited}");
}

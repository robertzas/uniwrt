//! The diagnostics runs' routes on the demo, whose simulated nodes answer the tools: a ping run
//! to its end and audited, a long one stopped, and what is refused.

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, post, request};

/// The run at `location` once it has ended.
async fn ended(demo: &Demo, location: &str) -> Value {
    let uri = location.trim_start_matches("/api");
    for _ in 0..100 {
        let run = json(demo.send(demo.get(uri)).await).await;
        if run["state"] != "running" {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the run never ended");
}

#[tokio::test]
async fn a_ping_runs_on_a_node_to_its_end() {
    let demo = Demo::start().await;
    let node = demo.ids[0];
    let body =
        json!({ "node": node.get(), "tool": { "tool": "ping", "target": "1.1.1.1", "count": 2 } });
    let res = demo
        .send(post("/v1/diag/runs", &body, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let run = json(res).await;
    assert_eq!(location, format!("/api/v1/diag/runs/{}", run["id"]));
    assert_eq!(run["state"], "running");
    assert_eq!(run["tool"]["family"], Value::Null);
    let run = ended(&demo, &location).await;
    assert_eq!(run["state"], "succeeded", "{run}");
    assert_eq!(run["exit_code"], 0);
    let lines: Vec<&str> = run["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["text"].as_str().unwrap())
        .collect();
    assert!(
        lines[1].starts_with("64 bytes from 1.1.1.1: seq=0"),
        "{lines:?}"
    );
    assert!(lines.contains(&"2 packets transmitted, 2 packets received, 0% packet loss"));
    let audited = demo.store.audit_log(1).await.unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert!(audited.starts_with("ping 1.1.1.1 on "), "{audited}");
    let list = json(demo.send(demo.get("/v1/diag/runs")).await).await;
    assert_eq!(list[0]["id"], run["id"]);
    assert_eq!(list[0]["lines"], json!([]));
}

#[tokio::test]
async fn an_admin_stops_a_run() {
    let demo = Demo::start().await;
    let body = json!({ "node": demo.ids[0].get(), "tool": { "tool": "ping", "target": "1.1.1.1", "count": 100 } });
    let run = json(
        demo.send(post("/v1/diag/runs", &body, Some(&demo.cookie)))
            .await,
    )
    .await;
    let uri = format!("/v1/diag/runs/{}", run["id"]);
    let res = demo
        .send(request(Method::DELETE, &uri, None, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let run = ended(&demo, &uri).await;
    assert_eq!(run["state"], "stopped");
    assert_eq!(run["error"], Value::Null, "stopped by an admin");
}

#[tokio::test]
async fn bad_requests_are_refused() {
    let demo = Demo::start().await;
    let node = demo.ids[0].get();
    let start = |body: Value| post("/v1/diag/runs", &body, Some(&demo.cookie));
    let res = demo
        .send(start(
            json!({ "node": node, "tool": { "tool": "ping", "target": "-f" } }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = demo
        .send(start(
            json!({ "node": node, "tool": { "tool": "sh", "command": "id" } }),
        ))
        .await;
    assert!(res.status().is_client_error());
    let res = demo
        .send(start(
            json!({ "node": 999, "tool": { "tool": "http", "url": "https://openwrt.org/" } }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = demo.send(demo.get("/v1/diag/runs/77")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = demo
        .send(request(
            Method::DELETE,
            "/v1/diag/runs/77",
            None,
            Some(&demo.cookie),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    for _ in 0..2 {
        let body =
            json!({ "node": node, "tool": { "tool": "ping", "target": "1.1.1.1", "count": 50 } });
        let res = demo.send(start(body)).await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }
    let body = json!({ "node": node, "tool": { "tool": "dns", "name": "openwrt.org" } });
    let res = demo.send(start(body)).await;
    assert_eq!(
        res.status(),
        StatusCode::CONFLICT,
        "two run on the node already"
    );
}

/// Installs iperf3 on `node`, through whichever package manager it has.
async fn install_iperf3(demo: &Demo, node: uniwrt_core::NodeId) {
    use uniwrt_engine::Fleet as _;
    for argv in [["opkg", "install"], ["apk", "add"]] {
        let request = uniwrt_node::ExecRequest::new(argv.into_iter().chain(["iperf3"]));
        if let Ok(output) = demo.engine.exec(node, request).await
            && output.status.success()
        {
            return;
        }
    }
    panic!("iperf3 didn't install");
}

#[tokio::test]
async fn a_throughput_test_runs_between_nodes() {
    let demo = Demo::start().await;
    let (node, peer) = (demo.ids[0], demo.ids[1]);
    install_iperf3(&demo, node).await;
    install_iperf3(&demo, peer).await;
    let peer_node = demo.store.node(peer).await.unwrap().unwrap();
    let body = json!({ "node": node.get(), "tool": { "tool": "iperf3", "peer": { "node": peer.get() }, "seconds": 1 } });
    let res = demo
        .send(post("/v1/diag/runs", &body, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let run = ended(&demo, &location).await;
    assert_eq!(run["state"], "succeeded", "{run}");
    let lines: Vec<&str> = run["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        lines[0],
        format!("Connecting to host {}, port 5201", peer_node.host)
    );
    assert_eq!(lines.last(), Some(&"iperf Done."));
    let audited = demo.store.audit_log(1).await.unwrap()[0]
        .detail
        .clone()
        .unwrap();
    assert!(
        audited.starts_with(&format!("iperf3 with {} on ", peer_node.name)),
        "{audited}"
    );
    let start = |body: Value| post("/v1/diag/runs", &body, Some(&demo.cookie));
    let res = demo
        .send(start(
            json!({ "node": node.get(), "tool": { "tool": "iperf3", "peer": { "node": 999 } } }),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = demo
        .send(start(
            json!({ "node": node.get(), "tool": { "tool": "iperf3", "peer": "controller" } }),
        ))
        .await;
    assert_eq!(
        res.status(),
        StatusCode::BAD_REQUEST,
        "the server is off in tests"
    );
}

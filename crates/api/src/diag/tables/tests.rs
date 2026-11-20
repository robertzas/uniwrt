//! The tables and the optional tools on the demo, whose simulated nodes answer the commands.

use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::testing::{Demo, json, post};

/// `uri` once the node's facts are in (409 before).
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
async fn the_gateway_s_tables_are_read() {
    let demo = Demo::start().await;
    let node = demo.ids[0].get();
    let neighbors = ready(&demo, &format!("/v1/nodes/{node}/diag/neighbors")).await;
    let neighbors = neighbors.as_array().unwrap();
    assert!(
        neighbors.iter().any(|row| row["address"] == "203.0.113.1"),
        "{neighbors:?}"
    );
    assert!(neighbors.iter().all(|row| row["state"].is_string()));
    let routes = ready(&demo, &format!("/v1/nodes/{node}/diag/routes")).await;
    assert_eq!(routes[0]["destination"], "default");
    assert_eq!(routes[0]["family"], "ipv4");
    assert_eq!(routes[0]["hops"][0]["via"], "203.0.113.1");
    let routes = routes.as_array().unwrap();
    assert!(routes.iter().any(|route| route["family"] == "ipv6"));
    let table = ready(&demo, &format!("/v1/nodes/{node}/diag/conntrack")).await;
    let connections = table["connections"].as_array().unwrap();
    assert_ne!(connections.len(), 0);
    assert_eq!(table["count"], connections.len());
    assert_eq!(table["listed"], true);
    assert_eq!(
        (table["max"].clone(), table["truncated"].clone()),
        (json!(16_384), json!(false))
    );
    assert_eq!(
        connections[0]["original"]["dport"], 22,
        "the controller's SSH"
    );
    let res = demo.send(demo.get("/v1/nodes/999/diag/routes")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_tools_are_listed_and_a_cable_test_names_its_package() {
    let demo = Demo::start().await;
    let node = demo.ids[0].get();
    let tools = ready(&demo, &format!("/v1/nodes/{node}/diag/tools")).await;
    assert_eq!(
        tools,
        json!([
            { "tool": "tcpdump", "installed": false, "package": "tcpdump-mini", "replaces": null },
            { "tool": "iperf3", "installed": false, "package": "iperf3", "replaces": null },
            { "tool": "ethtool", "installed": false, "package": "ethtool-full", "replaces": null },
        ])
    );
    let body = json!({ "node": node, "tool": { "tool": "cable_test", "port": "lan1" } });
    let run = json(
        demo.send(post("/v1/diag/runs", &body, Some(&demo.cookie)))
            .await,
    )
    .await;
    let uri = format!("/v1/diag/runs/{}", run["id"]);
    for _ in 0..100 {
        let run = json(demo.send(demo.get(&uri)).await).await;
        if run["state"] != "running" {
            assert_eq!(run["state"], "failed");
            assert_eq!(
                run["error"],
                "The device lacks the tool: install the ethtool-full package"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the cable test never ended");
}

#[tokio::test]
async fn a_port_name_is_checked() {
    let demo = Demo::start().await;
    let body = json!({ "node": demo.ids[0].get(), "tool": { "tool": "cable_test", "port": "-x" } });
    let res = demo
        .send(post("/v1/diag/runs", &body, Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

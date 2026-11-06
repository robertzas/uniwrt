//! The address plan's suggestions against a new site: the default LAN 10.0.0.0/24 in the
//! space 10.0.0.0/16.

use axum::http::{Method, StatusCode};
use serde_json::Value;

use crate::testing::{Demo, json, request};

async fn get(demo: &Demo, uri: &str) -> (StatusCode, Value) {
    let res = demo
        .send(request(Method::GET, uri, None, Some(&demo.cookie)))
        .await;
    let status = res.status();
    (status, json(res).await)
}

#[tokio::test]
async fn a_new_network_gets_the_slash_24_of_its_vlan() {
    let demo = Demo::start().await;
    let (status, network) = get(&demo, "/v1/ipam/network?purpose=iot").await;
    assert_eq!(status, StatusCode::OK, "{network}");
    assert_eq!(network["id"], "iot");
    assert_eq!(network["vlan"], 30);
    assert_eq!(network["ipv4"]["address"], "10.0.30.1/24");
    assert_eq!(network["ipv4"]["dhcp"]["start"], "10.0.30.100");
    let (_, guest) = get(&demo, "/v1/ipam/network?purpose=guest&vlan=77").await;
    assert_eq!(guest["ipv4"]["address"], "10.0.77.1/24");
    assert_eq!(guest["isolation"], true);
    for (uri, status) in [
        ("/v1/ipam/network?purpose=default", StatusCode::BAD_REQUEST),
        (
            "/v1/ipam/network?purpose=iot&vlan=0",
            StatusCode::BAD_REQUEST,
        ),
        ("/v1/ipam/network?purpose=lab", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(get(&demo, uri).await.0, status, "{uri}");
    }
}

#[tokio::test]
async fn fix_ip_keeps_the_current_address() {
    let demo = Demo::start().await;
    let (status, found) = get(&demo, "/v1/ipam/reservation?ip=10.0.0.150").await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["network"], "lan");
    assert_eq!(found["ip"], "10.0.0.150");
    assert_eq!(found["existing"], false);
    // The gateway's own address can't be reserved: the first of .32–.99 instead.
    let (_, found) = get(
        &demo,
        "/v1/ipam/reservation?ip=10.0.0.1&mac=02:00:00:00:00:99",
    )
    .await;
    assert_eq!(found["ip"], "10.0.0.32");
    let (status, _) = get(&demo, "/v1/ipam/reservation?ip=172.16.0.5").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

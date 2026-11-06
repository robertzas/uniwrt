//! The WAN wizard's routes against the simulated fleet, whose ISP serves DHCP on VLAN 201.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_model::{node::NodeMode, user::AuditAction};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::testing::{Demo, json, request};

/// Sends `body` to `uri` as the admin.
async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let res = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = res.status();
    (status, json(res).await)
}

/// Waits for job `id` to end.
async fn ended(demo: &Demo, id: &Value) -> Value {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let (_, job) = send(demo, Method::GET, &format!("/v1/jobs/{id}"), None).await;
            if job["state"] != "running" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the job ends")
}

fn wan(vlan: Option<u16>) -> Value {
    json!({
        "proto": {"kind": "dhcp", "vendor_class": null, "client_id": null, "send_options": [], "hostname": null},
        "vlan": vlan, "pcp": null, "mtu": null, "mac": null,
        "ipv6": {"mode": "dhcpv6", "reqprefix": null},
        "preset": "us-lumen-ipoe",
    })
}

#[tokio::test]
async fn presets_list_generic_ones_first() {
    let demo = Demo::start().await;
    let (status, presets) = send(&demo, Method::GET, "/v1/isp/presets", None).await;
    assert_eq!(status, StatusCode::OK);
    let presets = presets.as_array().unwrap();
    assert!(presets[0]["country"].is_null());
    let lumen = presets.iter().find(|p| p["id"] == "us-lumen-ipoe").unwrap();
    assert_eq!(lumen["ipv4"]["vlan"], 201);
    assert_eq!(lumen["ipv6"]["mode"], "auto");
}

#[tokio::test]
async fn a_wan_test_needs_a_managed_gateway_and_a_valid_wan() {
    let demo = Demo::start().await;
    let body = json!({"wan": wan(Some(201))});
    let (status, problem) = send(&demo, Method::POST, "/v1/wan/test", Some(&body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{problem}");
    assert!(problem["detail"].as_str().unwrap().contains("observe mode"));

    demo.store
        .set_node_mode(demo.ids[0], NodeMode::Managed)
        .await
        .unwrap();
    let mut bad = wan(Some(201));
    bad["pcp"] = json!(9);
    let bad = json!({"wan": bad, "password": "x"});
    let (status, problem) = send(&demo, Method::POST, "/v1/wan/test", Some(&bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["issues"][0]["path"], "wan/pcp");
    let sixrd = json!({"mode": "6rd", "prefix": "2602::/24", "border_relay": "0.0.0.0", "ipv4_mask_len": 0});
    let bad = json!({"wan": wan(Some(201)), "fallback": sixrd});
    let (status, problem) = send(&demo, Method::POST, "/v1/wan/test", Some(&bad)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["issues"][0]["path"], "wan/ipv6/border_relay");

    // The test harness's nodes take no logins: the job starts, then fails to reach the gateway.
    let (status, job) = send(&demo, Method::POST, "/v1/wan/test", Some(&body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["kind"], "wan_test");
    let job = ended(&demo, &job["id"]).await;
    assert_eq!(job["state"], "failed", "{job:#}");
    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(
        audit
            .iter()
            .any(|entry| entry.action == AuditAction::WanTest)
    );
}

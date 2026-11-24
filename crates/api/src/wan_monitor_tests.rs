//! The WAN quality monitor's routes over the demo: the simulated gateway's rounds and their
//! health, saving targets (checked, admins only, audited), and turning the monitor off.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_model::user::AuditAction;
use uniwrt_store::AuditRepo as _;

use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, body: Option<Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(
            method,
            "/v1/wan/monitor",
            body.as_ref(),
            Some(&demo.cookie),
        ))
        .await;
    let status = response.status();
    (status, json(response).await)
}

/// The monitor once `wanted` holds, within 10 s.
async fn until(demo: &Demo, wanted: impl Fn(&Value) -> bool) -> Value {
    let wait = async {
        loop {
            let (status, view) = send(demo, Method::GET, None).await;
            assert_eq!(status, StatusCode::OK, "{view}");
            if wanted(&view) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("the monitor within 10 s")
}

#[tokio::test]
async fn the_gateway_s_rounds_show_with_their_health() {
    let demo = Demo::start().await;
    let view = until(&demo, |view| !view["round"].is_null()).await;
    assert_eq!(view["health"], json!({ "state": "up" }));
    assert_eq!(view["gateway"], json!(demo.ids[0].get()));
    assert_eq!(view["settings"]["targets"][1]["id"], "cloudflare");
    let results = view["round"]["results"].as_array().unwrap();
    let cloudflare = results.iter().find(|r| r["key"] == "cloudflare").unwrap();
    assert_eq!(cloudflare["sent"], 10);
    assert!(results.iter().any(|r| r["key"] == "isp-dns@100.64.0.53"));
}

#[tokio::test]
async fn targets_are_saved_checked_and_audited() {
    let demo = Demo::start().await;
    let (_, view) = send(&demo, Method::GET, None).await;
    let mut settings = view["settings"].clone();
    settings["targets"] = json!([
        { "id": "first-hop", "name": " ISP ", "probe": { "kind": "icmp", "host": null } },
        { "id": "home", "name": "Home server", "probe": { "kind": "icmp", "host": "203.0.113.9" } },
    ]);
    let (status, saved) = send(&demo, Method::PUT, Some(settings.clone())).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["settings"]["targets"][0]["name"], "ISP");
    until(&demo, |view| {
        view["round"]["results"]
            .as_array()
            .is_some_and(|results| results.iter().any(|r| r["key"] == "home"))
    })
    .await;
    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(
        audit
            .iter()
            .any(|entry| entry.action == AuditAction::WanMonitor
                && entry.detail.as_deref() == Some("on, probing first-hop, home")),
        "{audit:?}"
    );

    settings["targets"][1]["probe"]["host"] = json!("not a host!");
    let (status, error) = send(&demo, Method::PUT, Some(settings.clone())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");

    settings["enabled"] = json!(false);
    settings["targets"][1]["probe"]["host"] = json!("203.0.113.9");
    let (status, _) = send(&demo, Method::PUT, Some(settings)).await;
    assert_eq!(status, StatusCode::OK);
    until(&demo, |view| {
        view["settings"]["enabled"] == false && view["round"].is_null()
    })
    .await;
}

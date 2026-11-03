//! The alerts routes over the demo's store and alert engine: alerts stored by hand, listed and
//! acknowledged; the built-in rules edited, refused deletion and reset; a rule of the user's;
//! maintenance windows.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::{EventId, NodeId, RuleId, Timestamp};
use uniwrt_model::{
    alerts::{AlertCause, AlertState, AlertWrite},
    events::EventLevel,
};
use uniwrt_store::AlertRepo as _;

use crate::testing::{Demo, json, post, request};

fn state(subject: &str, started: i64, resolved: bool) -> AlertState {
    AlertState {
        name: "Device offline".to_owned(),
        level: EventLevel::Critical,
        subject: subject.to_owned(),
        node: Some(NodeId::new(1)),
        client: None,
        started_at: Timestamp::from_unix_ms(started),
        last_at: Timestamp::from_unix_ms(started),
        resolved_at: resolved.then(|| Timestamp::from_unix_ms(started + 1_000)),
        resolution: None,
        count: 1,
        silenced: false,
        cause: AlertCause::Event {
            event: EventId::new(1),
            kind: "node_offline".to_owned(),
            matches: 1,
            condition: None,
        },
    }
}

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    let body = if status == StatusCode::NO_CONTENT {
        Value::Null
    } else {
        json(response).await
    };
    (status, body)
}

#[tokio::test]
async fn alerts_are_listed_and_acknowledged() {
    let demo = Demo::start_with(false).await;
    let rule = RuleId::new(1);
    let stored = demo
        .store
        .record_alerts(vec![
            AlertWrite::Open {
                rule,
                state: state("node:1/offline", 1_000, true),
            },
            AlertWrite::Open {
                rule,
                state: state("node:1", 2_000, false),
            },
        ])
        .await
        .unwrap();
    let response = demo.send(demo.get("/v1/alerts?active=true")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = json(response).await;
    assert_eq!(page["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(page["alerts"][0]["subject"], "node:1");
    let all = json(demo.send(demo.get("/v1/alerts?limit=1")).await).await;
    assert_eq!(all["next"], format!("2000:{}", stored[1].id));
    let bad = demo.send(demo.get("/v1/alerts?min=loud")).await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    let uri = format!("/v1/alerts/{}/acknowledge", stored[1].id);
    let (status, acked) = send(&demo, Method::POST, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(acked["acknowledged"]["username"], "admin");
    // A resolved alert is not acknowledged.
    let uri = format!("/v1/alerts/{}/acknowledge", stored[0].id);
    let (status, _) = send(&demo, Method::POST, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn built_in_rules_are_edited_and_reset_but_never_deleted() {
    let demo = Demo::start_with(false).await;
    let list = json(demo.send(demo.get("/v1/alert-rules")).await).await;
    let rules = list["rules"].as_array().unwrap();
    assert!(rules.len() >= 15);
    let offline = rules
        .iter()
        .find(|rule| rule["preset"] == "node-offline")
        .unwrap()
        .clone();
    assert_eq!(offline["modified"], false);
    assert_eq!(offline["when"]["type"], "event");
    let uri = format!("/v1/alert-rules/{}", offline["id"]);
    let mut spec = offline.clone();
    spec["enabled"] = json!(false);
    let (status, edited) = send(&demo, Method::PUT, &uri, Some(&spec)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (edited["enabled"].clone(), edited["modified"].clone()),
        (json!(false), json!(true))
    );
    let (status, _) = send(&demo, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, reset) = send(&demo, Method::POST, &format!("{uri}/reset"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (reset["enabled"].clone(), reset["modified"].clone()),
        (json!(true), json!(false))
    );
}

#[tokio::test]
async fn users_rules_are_checked_created_and_deleted() {
    let demo = Demo::start_with(false).await;
    let mut rule = json!({
        "name": "Gateway busy", "enabled": true, "level": "warning", "cooldown_s": 600,
        "when": { "type": "metric", "metric": "load", "entities": "node:1", "op": "above",
                  "threshold": 2.0, "clear": 1.5, "for_s": 300 },
    });
    let (status, created) = send(&demo, Method::POST, "/v1/alert-rules", Some(&rule)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        (created["preset"].clone(), created["modified"].clone()),
        (Value::Null, json!(false))
    );
    // Load is not a client's series.
    rule["when"]["entities"] = json!("client:*");
    let (status, problem) = send(&demo, Method::POST, "/v1/alert-rules", Some(&rule)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        problem["detail"].as_str().unwrap().contains("load"),
        "{problem}"
    );
    let uri = format!("/v1/alert-rules/{}", created["id"]);
    let (status, _) = send(&demo, Method::POST, &format!("{uri}/reset"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&demo, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&demo, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn maintenance_windows_are_scheduled_and_deleted() {
    let demo = Demo::start_with(false).await;
    let now = uniwrt_core::Clock::now(&uniwrt_core::SystemClock).as_unix_ms();
    let window = |from: i64, to: i64| json!({ "starts_at": now + from, "ends_at": now + to, "nodes": [1], "note": "new firmware" });
    let response = demo
        .send(post("/v1/maintenance", &window(0, -1), Some(&demo.cookie)))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let (status, created) = send(
        &demo,
        Method::POST,
        "/v1/maintenance",
        Some(&window(0, 3_600_000)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let list = json(demo.send(demo.get("/v1/maintenance")).await).await;
    assert_eq!(list["windows"][0]["note"], "new firmware");
    let uri = format!("/v1/maintenance/{}", created["id"]);
    assert_eq!(
        send(&demo, Method::DELETE, &uri, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&demo, Method::DELETE, &uri, None).await.0,
        StatusCode::NOT_FOUND
    );
}

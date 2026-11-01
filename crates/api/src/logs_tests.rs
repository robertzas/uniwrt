//! The logs routes over the demo's store, with entries recorded by hand.

use axum::http::{Method, StatusCode};
use serde_json::json;
use uniwrt_core::{Clock as _, NodeId, SystemClock, Timestamp};
use uniwrt_model::log::{LogEntry, LogVia, NewLog, Severity};
use uniwrt_store::LogRepo as _;

use crate::testing::{Demo, json, post, request};

fn new(
    node: Option<NodeId>,
    at: Timestamp,
    severity: Severity,
    program: &str,
    text: &str,
) -> NewLog {
    NewLog {
        received: at,
        node,
        host: node.is_none().then(|| "switch".to_owned()),
        via: LogVia::Agent,
        entry: LogEntry {
            seq: None,
            time: at,
            facility: 3,
            severity,
            program: Some(program.to_owned()),
            pid: None,
            message: text.to_owned(),
        },
        pattern: None,
    }
}

async fn seeded() -> (Demo, Timestamp) {
    let demo = Demo::start_with(false).await;
    let now = SystemClock.now();
    let ago = |secs: u64| now.saturating_sub(std::time::Duration::from_secs(secs));
    let (gw, ap) = (Some(demo.ids[0]), Some(demo.ids[1]));
    demo.store
        .record_logs(vec![
            new(
                gw,
                ago(300),
                Severity::Info,
                "dnsmasq-dhcp",
                "DHCPACK(br-lan) 10.0.0.20 phone",
            ),
            new(
                ap,
                ago(200),
                Severity::Warning,
                "hostapd",
                "DFS-RADAR-DETECTED freq=5500",
            ),
            new(
                None,
                ago(100),
                Severity::Error,
                "sshd",
                "Failed password for root",
            ),
            new(
                gw,
                ago(10),
                Severity::Notice,
                "netifd",
                "Interface 'wan' is now up",
            ),
        ])
        .await
        .unwrap();
    (demo, now)
}

fn programs(body: &serde_json::Value) -> Vec<&str> {
    body["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["program"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn entries_are_found_a_page_at_a_time() {
    let (demo, _) = seeded().await;
    let body = json(demo.send(demo.get("/v1/logs")).await).await;
    assert_eq!(
        programs(&body),
        ["netifd", "sshd", "hostapd", "dnsmasq-dhcp"]
    );
    assert!(body["next"].is_null());
    assert_eq!(body["records"][1]["host"], "switch");

    let gw = demo.ids[0];
    let uri = format!("/v1/logs?nodes={gw}&q=phone");
    assert_eq!(
        programs(&json(demo.send(demo.get(&uri)).await).await),
        ["dnsmasq-dhcp"]
    );
    let body = json(
        demo.send(demo.get("/v1/logs?min=warning&hosts=switch,x"))
            .await,
    )
    .await;
    assert_eq!(programs(&body), ["sshd"]);
    let body = json(
        demo.send(demo.get("/v1/logs?programs=hostapd,netifd&limit=1"))
            .await,
    )
    .await;
    assert_eq!(programs(&body), ["netifd"]);
    let next = body["next"].as_str().unwrap().to_owned();
    let uri = format!("/v1/logs?programs=hostapd,netifd&limit=1&before={next}");
    let body = json(demo.send(demo.get(&uri)).await).await;
    assert_eq!(programs(&body), ["hostapd"]);

    // Context around the radar entry, and the sources.
    let id = body["records"][0]["id"].as_i64().unwrap();
    let body = json(
        demo.send(demo.get(&format!("/v1/logs/{id}/context?around=1")))
            .await,
    )
    .await;
    assert_eq!(programs(&body), ["dnsmasq-dhcp", "hostapd", "sshd"]);
    let res = demo.send(demo.get("/v1/logs/9999/context")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = json(demo.send(demo.get("/v1/logs/sources")).await).await;
    assert_eq!(body["sources"]["hosts"], json!(["switch"]));
    assert_eq!(body["sources"]["programs"].as_array().unwrap().len(), 4);
    let body = json(demo.send(demo.get("/v1/logs/patterns")).await).await;
    assert!(
        body["patterns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "dfs-radar")
    );
}

#[tokio::test]
async fn the_histogram_counts_by_severity() {
    let (demo, now) = seeded().await;
    let from = now.as_unix_ms() - 600_000;
    let to = now.as_unix_ms();
    let uri = format!("/v1/logs/histogram?from={from}&to={to}&buckets=10");
    let body = json(demo.send(demo.get(&uri)).await).await;
    assert_eq!(body["step_ms"], 60_000);
    let buckets = body["buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 10);
    let total: u64 = buckets
        .iter()
        .flat_map(|b| b["counts"].as_array().unwrap())
        .map(|c| c.as_u64().unwrap())
        .sum();
    assert_eq!(total, 4);
    // The last hour by default; a filter applies.
    let body = json(demo.send(demo.get("/v1/logs/histogram?min=error")).await).await;
    assert_eq!(body["buckets"].as_array().unwrap().len(), 60);
    let errors: u64 = body["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["counts"][3].as_u64().unwrap())
        .sum();
    assert_eq!(errors, 1);
}

#[tokio::test]
async fn searches_are_saved_listed_and_deleted() {
    let (demo, _) = seeded().await;
    let cookie = Some(demo.cookie.as_str());
    let body = json!({"name": " Radar ", "filter": {"min": "warning", "text": "radar"}});
    let res = demo.send(post("/v1/logs/searches", &body, cookie)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let saved = json(res).await;
    assert_eq!(saved["name"], "Radar");
    assert!(saved["created_by"].is_number());
    let res = demo.send(post("/v1/logs/searches", &body, cookie)).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    for bad in [
        json!({"name": "", "filter": {}}),
        json!({"name": "x", "filter": {"min": "loud"}}),
    ] {
        let res = demo.send(post("/v1/logs/searches", &bad, cookie)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    let list = json(demo.send(demo.get("/v1/logs/searches")).await).await;
    assert_eq!(list["searches"][0]["filter"]["text"], "radar");
    let uri = format!("/v1/logs/searches/{}", saved["id"]);
    let res = demo.send(request(Method::DELETE, &uri, None, cookie)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = demo.send(request(Method::DELETE, &uri, None, cookie)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn malformed_queries_are_refused() {
    let demo = Demo::start_with(false).await;
    let long = "x".repeat(300);
    let many = vec!["1"; 40].join(",");
    for uri in [
        "/v1/logs?nodes=x".to_owned(),
        "/v1/logs?nodes=0".to_owned(),
        "/v1/logs?min=loud".to_owned(),
        "/v1/logs?limit=0".to_owned(),
        "/v1/logs?limit=5000".to_owned(),
        "/v1/logs?before=nope".to_owned(),
        format!("/v1/logs?q={long}"),
        format!("/v1/logs?nodes={many}"),
        "/v1/logs/histogram?from=10&to=10".to_owned(),
        "/v1/logs/histogram?buckets=0".to_owned(),
        "/v1/logs/1/context?around=1000".to_owned(),
    ] {
        let res = demo.send(demo.get(&uri)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
    // A session is needed.
    let res = demo
        .send(request(Method::GET, "/v1/logs", None, None))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

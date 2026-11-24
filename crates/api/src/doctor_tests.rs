//! The Connection Doctor's route over the demo: a client's DHCP trouble found through the log
//! lines that name it (the full-text search on its address), and the query's bounds.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::Value;
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::log::{LogEntry, LogVia, NewLog, Severity};
use uniwrt_store::LogRepo as _;

use crate::testing::{Demo, json, request};

async fn get(demo: &Demo, uri: &str) -> (StatusCode, Value) {
    let response = demo
        .send(request(Method::GET, uri, None, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

/// A client the demo's fleet knows.
async fn known_client(demo: &Demo) -> MacAddr {
    let wait = async {
        loop {
            let (_, clients) = get(demo, "/v1/clients").await;
            if let Some(mac) = clients
                .as_array()
                .unwrap()
                .iter()
                .find_map(|c| c["mac"].as_str()?.parse().ok())
            {
                return mac;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("a client within 10 s")
}

fn line(demo: &Demo, minutes_ago: u64, program: &str, message: String) -> NewLog {
    let at = demo
        .api
        .now()
        .saturating_sub(Duration::from_mins(minutes_ago));
    NewLog {
        received: at,
        node: Some(NodeId::new(1)),
        host: None,
        via: LogVia::Agent,
        entry: LogEntry {
            seq: None,
            time: at,
            facility: 3,
            severity: Severity::Info,
            program: Some(program.to_owned()),
            pid: Some(1),
            message,
        },
        pattern: None,
    }
}

#[tokio::test]
async fn the_doctor_finds_a_client_s_dhcp_trouble_in_its_log_lines() {
    let demo = Demo::start().await;
    let mac = known_client(&demo).await;
    let other: MacAddr = "02:99:00:00:00:01".parse().unwrap();
    demo.store
        .record_logs(vec![
            line(
                &demo,
                30,
                "dnsmasq-dhcp",
                format!("DHCPDISCOVER(br-lan) {mac} no address available"),
            ),
            line(
                &demo,
                20,
                "dnsmasq-dhcp",
                format!("DHCPDISCOVER(br-lan) {mac} no address available"),
            ),
            line(
                &demo,
                10,
                "dnsmasq-dhcp",
                format!("DHCPDISCOVER(br-lan) {mac} no address available"),
            ),
            line(
                &demo,
                10,
                "dnsmasq-dhcp",
                format!("DHCPDISCOVER(br-lan) {other} no address available"),
            ),
            // Older than the span asked for below.
            line(&demo, 180, "kernel", format!("long ago {mac}")),
        ])
        .await
        .unwrap();

    let (status, diagnosis) = get(&demo, &format!("/v1/clients/{mac}/doctor?hours=2")).await;
    assert_eq!(status, StatusCode::OK, "{diagnosis}");
    let dhcp: Vec<_> = diagnosis["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "dhcp")
        .collect();
    assert_eq!(dhcp.len(), 3, "{diagnosis}");
    assert!(dhcp.iter().all(|entry| entry["step"] == "no_address"));
    assert!(
        !diagnosis["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "log")
    );
    let issue = diagnosis["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["problem"]["kind"] == "dhcp_failures")
        .unwrap_or_else(|| panic!("no DHCP issue in {diagnosis}"));
    assert_eq!(issue["problem"]["failures"], 3);
    assert_eq!(issue["problem"]["reason"], "pool_full");
    assert_eq!(issue["fixes"][0]["fix"], "widen_dhcp_pool");

    // A day back takes the older line in.
    let (_, day) = get(&demo, &format!("/v1/clients/{mac}/doctor")).await;
    assert!(
        day["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "log" && entry["program"] == "kernel")
    );
}

#[tokio::test]
async fn the_doctor_refuses_bad_spans_and_unknown_clients() {
    let demo = Demo::start().await;
    let mac = known_client(&demo).await;
    for hours in ["0", "169", "x"] {
        let (status, _) = get(&demo, &format!("/v1/clients/{mac}/doctor?hours={hours}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "hours={hours}");
    }
    let (status, _) = get(&demo, "/v1/clients/02:99:00:00:00:02/doctor").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&demo, "/v1/clients/nope/doctor").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

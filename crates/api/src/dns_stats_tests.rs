//! DNS query statistics' routes over the demo's store: totals, series and clients named as
//! `/clients` names them, the top domains, and pages of queries filtered and followed.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::Value;
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::dns_queries::{DnsHour, DnsQuery, Outcome};
use uniwrt_store::DnsQueryRepo as _;

use crate::testing::{Demo, json, request};

async fn get(demo: &Demo, uri: &str) -> (StatusCode, Value) {
    let response = demo
        .send(request(Method::GET, uri, None, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

/// A client `/clients` names, with an address: its MAC, address and name.
async fn named_client(demo: &Demo) -> (MacAddr, String, String) {
    let wait = async {
        loop {
            let (_, clients) = get(demo, "/v1/clients").await;
            let found = clients.as_array().unwrap().iter().find_map(|c| {
                let name = c["alias"].as_str().or(c["hostname"].as_str())?;
                let ip = c["ips"][0].as_str()?;
                Some((
                    c["mac"].as_str()?.parse().ok()?,
                    ip.to_owned(),
                    name.to_owned(),
                ))
            });
            if let Some(found) = found {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("a named client within 10 s")
}

fn query(at: Timestamp, client: &str, mac: Option<MacAddr>, domain: &str, o: Outcome) -> DnsQuery {
    DnsQuery {
        at,
        client: client.parse().unwrap(),
        mac,
        qtype: "A".to_owned(),
        domain: domain.to_owned(),
        outcome: o,
    }
}

/// The demo with a named client's queries and hours, a client's by address, and an unknown
/// client's: the named client's MAC, address and name.
async fn seeded() -> (Demo, MacAddr, String, String) {
    let demo = Demo::start().await;
    let (mac, ip, name) = named_client(&demo).await;
    let now = demo.api.now();
    let ago = |hours| now.saturating_sub(Duration::from_hours(hours));
    let stranger = "192.0.2.99";
    demo.store
        .record_dns(
            vec![
                query(ago(1), &ip, Some(mac), "ads.example", Outcome::Blocked),
                query(ago(1), &ip, Some(mac), "www.example", Outcome::Cached),
                query(ago(0), &ip, None, "www.example", Outcome::Forwarded),
                query(ago(0), stranger, None, "mail.example", Outcome::Local),
            ],
            vec![
                DnsHour {
                    hour: ago(48),
                    client: mac.to_string(),
                    queries: 10,
                    blocked: 4,
                },
                DnsHour {
                    hour: ago(1),
                    client: mac.to_string(),
                    queries: 2,
                    blocked: 1,
                },
                DnsHour {
                    hour: ago(0),
                    client: ip.clone(),
                    queries: 1,
                    blocked: 0,
                },
                DnsHour {
                    hour: ago(0),
                    client: stranger.to_owned(),
                    queries: 1,
                    blocked: 0,
                },
            ],
        )
        .await
        .unwrap();
    (demo, mac, ip, name)
}

#[tokio::test]
async fn stats_sum_the_range_and_name_clients() {
    let (demo, mac, ip, name) = seeded().await;
    let stranger = "192.0.2.99";
    let (status, day) = get(&demo, "/v1/services/dns/stats").await;
    assert_eq!(status, StatusCode::OK, "{day}");
    assert_eq!(day["range"], "24h");
    assert_eq!(
        (day["queries"].as_u64(), day["blocked"].as_u64()),
        (Some(4), Some(1))
    );
    let hours: u64 = day["hours"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["queries"].as_u64().unwrap())
        .sum();
    assert_eq!(hours, 4);
    let clients = day["clients"].as_array().unwrap();
    assert_eq!(clients[0]["client"], mac.to_string(), "{day}");
    assert_eq!(clients[0]["mac"], mac.to_string());
    assert_eq!(clients[0]["name"], name.as_str());
    let by_address = clients.iter().find(|c| c["client"] == ip.as_str()).unwrap();
    assert_eq!(by_address["mac"], Value::Null);
    assert_eq!(
        by_address["name"],
        name.as_str(),
        "named by who holds the address"
    );
    let unknown = clients.iter().find(|c| c["client"] == stranger).unwrap();
    assert_eq!(unknown["name"], Value::Null);
    assert_eq!(day["top_blocked"][0]["domain"], "ads.example");
    assert_eq!(day["top_domains"][0]["domain"], "www.example");
    assert_eq!(day["top_domains"][0]["queries"], 2);

    let (_, week) = get(&demo, "/v1/services/dns/stats?range=7d").await;
    assert_eq!(
        (week["queries"].as_u64(), week["blocked"].as_u64()),
        (Some(14), Some(5))
    );
    let (status, _) = get(&demo, "/v1/services/dns/stats?range=1y").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pages_filter_and_follow() {
    let (demo, mac, ip, _) = seeded().await;
    let page = |query: &str| format!("/v1/services/dns/queries?{query}");
    let domains = |page: &Value| -> Vec<String> {
        page["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|q| q["domain"].as_str().unwrap().to_owned())
            .collect()
    };
    let (status, mine) = get(&demo, &page(&format!("client={mac}"))).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(domains(&mine), ["www.example", "ads.example"]);
    let (_, by_ip) = get(&demo, &page(&format!("client={ip}"))).await;
    assert_eq!(
        domains(&by_ip),
        ["www.example"],
        "only queries without a MAC"
    );
    let (_, blocked) = get(&demo, &page("outcome=blocked")).await;
    assert_eq!(domains(&blocked), ["ads.example"]);
    let (_, text) = get(&demo, &page("domain=%20MAIL%20")).await;
    assert_eq!(domains(&text), ["mail.example"]);

    let (_, first) = get(&demo, &page("limit=3")).await;
    assert_eq!(domains(&first).len(), 3);
    let next = first["next"].as_str().unwrap();
    let (_, rest) = get(&demo, &page(&format!("limit=3&before={next}"))).await;
    assert_eq!(domains(&rest), ["ads.example"]);
    assert_eq!(rest["next"], Value::Null);

    for bad in ["limit=0", "client=nobody", "before=soon", "outcome=lost"] {
        let (status, body) = get(&demo, &page(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
    }
}

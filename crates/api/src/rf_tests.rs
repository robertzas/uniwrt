//! The RF insight routes over the demo, whose scanner scans the simulated radios at once: the
//! view once every radio has a scan, scans queued and refused, the stored scans, the settings.

use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::testing::{Demo, json, request};

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

/// The view, once every radio has been scanned.
async fn scanned(demo: &Demo) -> Value {
    let wait = async {
        loop {
            let (status, view) = send(demo, Method::GET, "/v1/rf", None).await;
            assert_eq!(status, StatusCode::OK);
            let radios = view["radios"].as_array().cloned().unwrap_or_default();
            if radios.len() >= 6 && radios.iter().all(|r| !r["last"].is_null()) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(30), wait)
        .await
        .expect("every radio scanned within 30 s")
}

#[tokio::test]
async fn the_view_shows_what_the_scans_heard() {
    let demo = Demo::start_with(false).await;
    let view = scanned(&demo).await;
    let bands: Vec<_> = view["report"]["bands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["band"].clone())
        .collect();
    assert_eq!(bands, [json!("2g"), json!("5g")]);
    let five = &view["report"]["bands"][1];
    assert_ne!(
        five["matrix"].as_array().unwrap().len(),
        0,
        "the nodes hear each other"
    );
    let kinds: Vec<_> = five["networks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["kind"].as_str().unwrap().to_owned())
        .collect();
    assert!(kinds.contains(&"own".to_owned()) && kinds.contains(&"foreign".to_owned()));
    assert_eq!(
        view["settings"],
        json!({"scanning": true, "interval_h": 4, "dfs": "all", "nightly": true, "nightly_at_min": 210})
    );

    let gateway = demo.ids[0].get();
    let (status, scans) = send(
        &demo,
        Method::GET,
        &format!("/v1/rf/scans?node={gateway}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first = &scans[0];
    assert_eq!(first["node"], json!(gateway));
    assert!(first["networks"].as_u64().unwrap() > 0);
    let id = first["id"].as_i64().unwrap();
    let (status, scan) = send(&demo, Method::GET, &format!("/v1/rf/scans/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(scan["bsss"].as_array().unwrap().len(), 0);
    let (status, _) = send(&demo, Method::GET, "/v1/rf/scans/999999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&demo, Method::GET, "/v1/rf/scans?limit=0", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admins_queue_scans_and_change_the_settings() {
    let demo = Demo::start_with(false).await;
    let gateway = demo.ids[0].get();
    let one = json!({"node": gateway, "radio": "radio0"});
    let (status, queued) = send(&demo, Method::POST, "/v1/rf/scans", Some(&one)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{queued}");
    let queued = queued["queued"].as_array().unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0]["radio"], "radio0");
    let (status, _) = send(
        &demo,
        Method::POST,
        "/v1/rf/scans",
        Some(&json!({"radio": "radio0"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a radio needs its node");
    let (status, _) = send(
        &demo,
        Method::POST,
        "/v1/rf/scans",
        Some(&json!({"node": 999})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let bad = json!({"scanning": true, "interval_h": 30});
    let (status, _) = send(&demo, Method::PUT, "/v1/rf/settings", Some(&bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let late = json!({"scanning": true, "interval_h": 4, "nightly_at_min": 1440});
    let (status, _) = send(&demo, Method::PUT, "/v1/rf/settings", Some(&late)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let off = json!({"scanning": false, "interval_h": 8, "dfs": "non_weather", "nightly": false, "nightly_at_min": 60});
    let (status, body) = send(&demo, Method::PUT, "/v1/rf/settings", Some(&off)).await;
    assert_eq!((status, &body), (StatusCode::OK, &off));
    let (_, view) = send(&demo, Method::GET, "/v1/rf", None).await;
    assert_eq!(view["settings"], off);
}

#[tokio::test]
async fn the_plan_previews_each_band_s_radios() {
    let demo = Demo::start_with(false).await;
    scanned(&demo).await;
    let (status, plan) = send(&demo, Method::GET, "/v1/rf/plan", None).await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let bands = plan["bands"].as_array().unwrap();
    let names: Vec<_> = bands.iter().map(|b| b["band"].clone()).collect();
    assert_eq!(names, [json!("2g"), json!("5g")]);
    for band in bands {
        assert!(band["improvement"].is_number() && band["search"].is_string());
        let radios = band["radios"].as_array().unwrap();
        assert!(!radios.is_empty(), "{band}");
        for radio in radios {
            assert!(radio["planned"]["channel"].is_u64(), "{radio}");
            assert!(radio["after"]["total"].is_number(), "{radio}");
            assert!(radio["reasons"].is_array(), "{radio}");
        }
    }
}

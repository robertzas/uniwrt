//! The adoption and job routes through the router, against the simulated fleet's API: request
//! checks, a probe and an adoption of a node that cannot be reached (the job records why), the
//! audit, and a controller without a dial, where nothing can be adopted.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uniwrt_core::SystemClock;
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_model::user::AuditAction;
use uniwrt_node::ControllerKey;
use uniwrt_sim::SimLink;
use uniwrt_store::{AuditRepo as _, Store};

use crate::testing::{self, Demo, json as body, post, router_send};

/// A loopback port where nothing listens: connecting is refused at once.
async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

fn host_key() -> String {
    ControllerKey::generate().unwrap().authorized_key().unwrap()
}

#[tokio::test]
async fn requests_are_checked_before_anything_is_tried() {
    let demo = Demo::start().await;
    let cookie = Some(demo.cookie.as_str());
    let bad = [
        ("/v1/adopt/probe", json!({ "host": "a b" })),
        ("/v1/adopt/probe", json!({ "host": "-oProxyCommand=x" })),
        ("/v1/adopt/probe", json!({ "host": "10.0.0.2", "port": 0 })),
        (
            "/v1/adopt/probe",
            json!({ "host": "10.0.0.2", "user": "admin" }),
        ),
        (
            "/v1/adopt",
            json!({ "host": "10.0.0.2", "host_key": "not a key" }),
        ),
        (
            "/v1/adopt",
            json!({ "host": "10.0.0.2", "host_key": host_key(), "remember_password": true }),
        ),
        (
            "/v1/adopt",
            json!({ "host": "10.0.0.2", "host_key": host_key(), "name": "x".repeat(65) }),
        ),
        (
            "/v1/adopt",
            json!({ "host": "10.0.0.2", "host_key": host_key(), "password": "p".repeat(1025) }),
        ),
    ];
    for (uri, request) in bad {
        let res = demo.send(post(uri, &request, cookie)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{request}");
    }
    let res = demo
        .send(post(
            "/v1/adopt/probe",
            &json!({ "host": "10.0.0.2" }),
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(demo.api.jobs().jobs(10).await.unwrap(), []);
}

#[tokio::test]
async fn an_unreachable_node_fails_the_probe_and_the_adoption_records_why() {
    let demo = Demo::start().await;
    let cookie = Some(demo.cookie.as_str());
    let port = closed_port().await;
    let target = json!({ "host": "127.0.0.1", "port": port });
    let res = demo.send(post("/v1/adopt/probe", &target, cookie)).await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    let problem = body(res).await;
    assert!(
        problem["detail"].as_str().unwrap().contains("cannot reach"),
        "{problem}"
    );

    let adopt = json!({
        "host": "127.0.0.1",
        "port": port,
        "host_key": host_key(),
        "password": "secret",
        "name": "attic",
        "role": "ap",
    });
    let res = demo.send(post("/v1/adopt", &adopt, cookie)).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let location = res.headers()[header::LOCATION].to_str().unwrap().to_owned();
    let job = body(res).await;
    assert_eq!(job["kind"], "adopt");
    assert_eq!(job["state"], "running");
    assert_eq!(job["target"], format!("root@127.0.0.1:{port}"));
    assert_eq!(location, format!("/api/v1/jobs/{}", job["id"]));
    assert!(!job.to_string().contains("secret"));

    let uri = format!("/v1/jobs/{}", job["id"]);
    let failed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = body(demo.send(demo.get(&uri)).await).await;
            if job["state"] != "running" {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the job ends");
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["error"]["code"], "unreachable");
    assert_eq!(failed["steps"][0]["state"], "failed");
    assert_eq!(failed["steps"][1]["state"], "pending");

    let list = body(demo.send(demo.get("/v1/jobs?limit=5")).await).await;
    assert_eq!(list, Value::Array(vec![failed]));
    for uri in ["/v1/jobs?limit=0", "/v1/jobs?limit=x", "/v1/jobs/x"] {
        let res = demo.send(demo.get(uri)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{uri}");
    }
    let res = demo.send(demo.get("/v1/jobs/999")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let audit = demo.store.audit_log(1).await.unwrap();
    assert_eq!(audit[0].action, AuditAction::Adopt);
    assert_eq!(audit[0].username.as_deref(), Some("admin"));
    assert_eq!(
        audit[0].detail.as_deref(),
        Some(format!("root@127.0.0.1:{port}, job {}", job["id"]).as_str())
    );
}

#[tokio::test]
async fn without_a_dial_nothing_is_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let engine: Engine<SimLink> = Engine::new(EngineConfig::default(), clock.clone());
    let api = testing::api_with(store, Arc::new(engine), clock, None).await;
    api.create_admin("admin", testing::PASSWORD).await.unwrap();
    let cookie = testing::login(&api).await;
    let probe = json!({ "host": "10.0.0.2" });
    let res = router_send(&api, post("/v1/adopt/probe", &probe, Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let adopt = json!({ "host": "10.0.0.2", "host_key": host_key(), "password": "x" });
    let res = router_send(&api, post("/v1/adopt", &adopt, Some(&cookie))).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert!(
        body(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("cannot reach nodes")
    );
}

#[tokio::test]
async fn a_batch_is_checked_whole_then_starts_a_job_per_node() {
    let demo = Demo::start().await;
    let cookie = Some(demo.cookie.as_str());
    let node = |port: u16| json!({ "host": "127.0.0.1", "port": port, "host_key": host_key() });
    let (a, b) = (closed_port().await, closed_port().await);
    let many: Vec<Value> = (1..=65).map(node).collect();
    let bad = [
        json!({ "nodes": [] }),
        json!({ "nodes": many }),
        json!({ "nodes": [node(a), node(a)], "password": "secret" }),
        json!({ "nodes": [node(a), { "host": "a b", "host_key": host_key() }] }),
        json!({ "nodes": [node(a)], "remember_password": true }),
    ];
    for request in bad {
        let res = demo.send(post("/v1/adopt/batch", &request, cookie)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{request}");
    }
    let res = demo
        .send(post(
            "/v1/adopt/batch",
            &json!({ "nodes": [node(a), { "host": "a b", "host_key": host_key() }] }),
            cookie,
        ))
        .await;
    assert_eq!(
        body(res).await["detail"],
        "Node 2: The host must be an IP address or a host name"
    );
    assert_eq!(demo.api.jobs().jobs(10).await.unwrap(), []);

    let batch = json!({ "nodes": [node(a), node(b)], "password": "secret" });
    let res = demo.send(post("/v1/adopt/batch", &batch, cookie)).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let result = body(res).await;
    assert!(!result.to_string().contains("secret"));
    assert_eq!(result["refused"], json!([]));
    let targets: Vec<&str> = result["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|job| job["target"].as_str().unwrap())
        .collect();
    assert_eq!(
        targets,
        [format!("root@127.0.0.1:{a}"), format!("root@127.0.0.1:{b}")]
    );
    let audit = demo.store.audit_log(5).await.unwrap();
    assert_eq!(audit.len(), 3, "the login and both adoptions");
}

#[tokio::test]
async fn adopted_nodes_are_not_discovered_and_nothing_is_probed_without_a_dial() {
    let demo = Demo::start_with(false).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !demo
        .engine
        .all()
        .iter()
        .all(|(_, view)| view.facts.is_some())
    {
        assert!(tokio::time::Instant::now() < deadline, "no facts");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let res = demo.send(demo.get("/v1/discovered")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body(res).await, json!({ "nodes": [], "scanning": false }));
    let batch = json!({ "nodes": [{ "host": "10.0.0.2", "host_key": host_key() }] });
    let res = demo
        .send(post("/v1/adopt/batch", &batch, Some(demo.cookie.as_str())))
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = router_send(
        &demo.api,
        Request::get("/v1/discovered").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

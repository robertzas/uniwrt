//! Zero-touch provisioning through the router, against the simulated fleet's API: hellos
//! checked, kept, limited and answered; pre-approvals used once; adopting and dismissing from
//! the inbox; the first-boot script.

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use uniwrt_model::user::AuditAction;
use uniwrt_node::ControllerKey;
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use super::hello::PER_WINDOW;
use crate::testing::{Demo, json as body, post, request};

fn hello(mac: &str) -> Value {
    json!({
        "mac": mac,
        "board_name": "glinet,gl-mt6000",
        "model": "GL.iNet GL-MT6000",
        "hostname": "uniwrt-0001",
        "release": "24.10.4",
        "host_key": ControllerKey::generate().unwrap().authorized_key().unwrap(),
        "role": "ap",
    })
}

async fn say(demo: &Demo, hello: &Value) -> (StatusCode, Value) {
    let res = demo.send(post("/v1/ztp/hello", hello, None)).await;
    let status = res.status();
    (status, body(res).await)
}

async fn inbox(demo: &Demo) -> Value {
    body(demo.send(demo.get("/v1/ztp/pending")).await).await
}

#[tokio::test]
async fn a_hello_needs_no_session_and_lands_in_the_inbox() {
    let demo = Demo::start().await;
    let first = hello("94:83:c4:00:00:01");
    assert_eq!(
        say(&demo, &first).await,
        (StatusCode::OK, json!({ "state": "pending" }))
    );
    // Again from the same device, with another key (a reflash): the latest is kept.
    let again = hello("94:83:c4:00:00:01");
    assert_eq!(say(&demo, &again).await.1, json!({ "state": "pending" }));

    let inbox = inbox(&demo).await;
    let pending = inbox["pending"].as_array().unwrap();
    assert_eq!(pending.len(), 1);
    let device = &pending[0];
    assert_eq!(device["mac"], "94:83:c4:00:00:01");
    assert_eq!(device["address"], "192.168.1.20", "the request's address");
    // Without its comment.
    let sent = again["host_key"].as_str().unwrap();
    assert_eq!(
        device["host_key"].as_str(),
        sent.rsplit_once(' ').map(|(key, _)| key)
    );
    assert!(
        device["fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("SHA256:")
    );
    assert_eq!(device["job"], Value::Null);
    assert_eq!(inbox["approvals"], json!([]));

    // An adopted node's MAC is told so, and leaves the inbox.
    let gateway = demo.store.node(demo.ids[0]).await.unwrap().unwrap();
    let adopted = hello(&gateway.mac.to_string());
    assert_eq!(say(&demo, &adopted).await.1, json!({ "state": "adopted" }));
}

#[tokio::test]
async fn bad_hellos_are_refused() {
    let demo = Demo::start().await;
    let with = |field: &str, value: Value| {
        let mut body = hello("94:83:c4:00:00:01");
        body[field] = value;
        body
    };
    for bad in [
        with("mac", json!("01:00:5e:00:00:01")),
        with("mac", json!("00:00:00:00:00:00")),
        with("mac", json!("nope")),
        with("host_key", json!("not a key")),
        with("board_name", json!("")),
        with("model", json!("x".repeat(129))),
        with("hostname", json!("a\nb")),
        with("extra", json!(1)),
    ] {
        let (status, _) = say(&demo, &bad).await;
        assert!(status.is_client_error(), "{bad}: {status}");
    }
    // Over the body limit.
    let big = with("model", json!("x".repeat(5000)));
    assert!(say(&demo, &big).await.0.is_client_error());
    assert_eq!(inbox(&demo).await["pending"], json!([]));
}

#[tokio::test]
async fn hellos_are_limited_per_client() {
    let demo = Demo::start().await;
    let body = hello("94:83:c4:00:00:01");
    for _ in 0..PER_WINDOW {
        assert_eq!(say(&demo, &body).await.0, StatusCode::OK);
    }
    let res = demo.send(post("/v1/ztp/hello", &body, None)).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(res.headers().contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn a_pre_approval_adopts_at_the_first_hello_and_only_once() {
    let demo = Demo::start().await;
    let cookie = Some(demo.cookie.as_str());
    let mac = "94:83:c4:00:00:07";
    let approval = json!({ "name": "Attic", "role": "bridge" });
    let res = demo
        .send(request(
            Method::PUT,
            &format!("/v1/ztp/approvals/{mac}"),
            Some(&approval),
            cookie,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let approved = body(res).await;
    assert_eq!(
        (approved["name"].as_str(), approved["role"].as_str()),
        (Some("Attic"), Some("bridge"))
    );
    assert_eq!(inbox(&demo).await["approvals"].as_array().unwrap().len(), 1);

    // The first hello starts the adoption (192.168.1.20 isn't a simulated node: it fails).
    assert_eq!(say(&demo, &hello(mac)).await.1["state"], "adopting");
    let inbox_now = inbox(&demo).await;
    assert_eq!(inbox_now["approvals"], json!([]), "used up");
    let job = inbox_now["pending"][0]["job"].as_i64().unwrap();
    let audit = demo.store.audit_log(10).await.unwrap();
    let entry = audit
        .iter()
        .find(|entry| entry.action == AuditAction::Adopt)
        .unwrap();
    assert!(
        entry
            .detail
            .as_deref()
            .unwrap()
            .contains("pre-approved by admin")
    );
    assert_eq!(entry.user_id, None);

    let failed = loop {
        let job = body(demo.send(demo.get(&format!("/v1/jobs/{job}"))).await).await;
        if job["state"] != "running" {
            break job;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(failed["state"], "failed");
    // Not adopted, and the approval is gone: it waits in the inbox.
    assert_eq!(say(&demo, &hello(mac)).await.1["state"], "pending");
}

#[tokio::test]
async fn admins_adopt_dismiss_and_withdraw() {
    let demo = Demo::start().await;
    let cookie = Some(demo.cookie.as_str());
    let mac = "94:83:c4:00:00:01";
    say(&demo, &hello(mac)).await;

    let adopt = format!("/v1/ztp/pending/{mac}/adopt");
    let res = demo
        .send(post(&adopt, &json!({ "name": "Garage" }), cookie))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let job = body(res).await;
    assert_eq!(job["kind"], "adopt");
    assert_eq!(job["target"], "root@192.168.1.20:22");
    assert_eq!(inbox(&demo).await["pending"][0]["job"], job["id"]);
    let res = demo
        .send(post(&adopt, &json!({ "name": "" }), cookie))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let unknown = demo
        .send(post(
            "/v1/ztp/pending/94:83:c4:00:00:09/adopt",
            &json!({}),
            cookie,
        ))
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let dismiss = format!("/v1/ztp/pending/{mac}");
    let res = demo
        .send(request(Method::DELETE, &dismiss, None, cookie))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = demo
        .send(request(Method::DELETE, &dismiss, None, cookie))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let approval = format!("/v1/ztp/approvals/{mac}");
    demo.send(request(Method::PUT, &approval, Some(&json!({})), cookie))
        .await;
    let res = demo
        .send(request(Method::DELETE, &approval, None, cookie))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = demo
        .send(request(Method::DELETE, &approval, None, cookie))
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    // An adopted node's MAC can't be pre-approved.
    let gateway = demo.store.node(demo.ids[0]).await.unwrap().unwrap();
    let path = format!("/v1/ztp/approvals/{}", gateway.mac);
    let res = demo
        .send(request(Method::PUT, &path, Some(&json!({})), cookie))
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let audit = demo.store.audit_log(20).await.unwrap();
    let ztp: Vec<_> = audit
        .iter()
        .filter(|entry| entry.action == AuditAction::Ztp)
        .filter_map(|entry| entry.detail.as_deref())
        .collect();
    assert_eq!(ztp.len(), 3, "{ztp:?}");

    // Without a session, none of it.
    let res = demo.send(post(&adopt, &json!({}), None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_script_calls_home_where_the_admin_reached_the_controller() {
    let demo = Demo::start().await;
    let res = demo.send(demo.get("/v1/ztp/script?role=bridge")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"99-uniwrt\""
    );
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let script = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(script.starts_with("#!/bin/sh\n"));
    // The test client addressed uniwrt.lan:8443: no IP address to fall back to.
    assert!(
        script.contains("\nURLS='https://uniwrt.lan:8443'\n"),
        "{script}"
    );
    assert!(script.contains("\nROLE='bridge'\n"));
    let res = demo.send(demo.get("/v1/ztp/script?role=router")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

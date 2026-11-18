use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uniwrt_core::JobId;
use uniwrt_model::{job::JobState, user::AuditAction, ztp::ZtpApproval};
use uniwrt_node::ControllerKey;
use uniwrt_store::{AuditRepo as _, JobRepo as _, NodeRepo as _, ZtpRepo as _};

use crate::testing::{Demo, json as body, post, request};

async fn ended(demo: &Demo, job: &Value) -> uniwrt_model::job::Job {
    let id = JobId::new(job["id"].as_i64().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = demo.store.job(id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the job ends")
}

#[tokio::test]
async fn a_replace_adopts_into_the_record_and_an_unreachable_device_changes_nothing() {
    let demo = Demo::start().await;
    let ap = demo.ids[1];
    let before = demo.store.node(ap).await.unwrap().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let new = json!({
        "host": "127.0.0.1",
        "port": port,
        "host_key": ControllerKey::generate().unwrap().authorized_key().unwrap(),
        "password": "secret",
    });
    let uri = format!("/v1/nodes/{ap}/replace");
    let res = demo
        .send(request(Method::POST, &uri, Some(&new), Some(&demo.cookie)))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let job = body(res).await;
    assert_eq!(
        (job["kind"].as_str(), job["node"].as_i64()),
        (Some("replace"), Some(ap.get()))
    );
    assert_eq!(job["steps"][0]["name"], "connect");
    assert!(!job.to_string().contains("secret"));
    let failed = ended(&demo, &job).await;
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(demo.store.node(ap).await.unwrap().unwrap().mac, before.mac);

    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(
        audit
            .iter()
            .any(|entry| entry.action == AuditAction::Replace
                && entry
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.starts_with(&format!(
                        "replaced with the device at 127.0.0.1 the device of {}",
                        before.name
                    )))),
        "{audit:?}"
    );

    // Checked as an adoption is, and only for nodes that exist.
    let send = |uri: String, body: Value| {
        let demo = &demo;
        async move {
            demo.send(request(Method::POST, &uri, Some(&body), Some(&demo.cookie)))
                .await
                .status()
        }
    };
    let bad = json!({ "host": "10.0.0.9", "host_key": "not a key" });
    assert_eq!(send(uri.clone(), bad).await, StatusCode::BAD_REQUEST);
    let extra = json!({ "host": "10.0.0.9", "host_key": new["host_key"], "name": "x" });
    assert_eq!(send(uri, extra).await, StatusCode::BAD_REQUEST);
    let missing = json!({ "host": "10.0.0.9", "host_key": new["host_key"] });
    assert_eq!(
        send("/v1/nodes/999/replace".to_owned(), missing).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_reset_leaves_the_record_awaiting_its_device() {
    let demo = Demo::start().await;
    let send =
        |method: Method, uri: String| demo.send(request(method, &uri, None, Some(&demo.cookie)));
    let gateway = demo.ids[0];
    let res = send(Method::POST, format!("/v1/nodes/{gateway}/reset")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let ap = demo.ids[1];
    let res = send(Method::POST, format!("/v1/nodes/{ap}/reset")).await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let job = body(res).await;
    assert_eq!(job["kind"], "reset");
    let steps: Vec<_> = job["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(steps, ["connect", "remove_tool", "wipe", "record"]);
    // The demo's records pin no host key: the jobs' tests run it through.
    ended(&demo, &job).await;

    // The inbox lists the record's approval, which keeps its device out of the adopted ones.
    let node = demo.store.node(ap).await.unwrap().unwrap();
    demo.store
        .put_ztp_approval(ZtpApproval::awaiting(&node, demo.api.now(), None))
        .await
        .unwrap();
    let inbox = body(demo.send(demo.get("/v1/ztp/pending")).await).await;
    assert!(
        inbox["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["node"] == ap.get() && a["board_name"] == node.board_name.as_str()),
        "{inbox}"
    );
    let audit = demo.store.audit_log(10).await.unwrap();
    assert!(
        audit.iter().any(|e| e.action == AuditAction::Replace
            && e.detail.as_deref().is_some_and(|d| d.starts_with("reset "))),
        "{audit:?}"
    );
}

#[tokio::test]
async fn a_reset_node_s_hello_adopts_it_back_until_it_is_recorded() {
    let demo = Demo::start().await;
    let node = demo.store.node(demo.ids[1]).await.unwrap().unwrap();
    let hello = json!({
        "mac": node.mac.to_string(),
        "board_name": node.board_name,
        "model": "a model",
        "hostname": "OpenWrt",
        "release": "24.10.4",
        "host_key": ControllerKey::generate().unwrap().authorized_key().unwrap(),
    });
    let say = || async {
        let res = demo.send(post("/v1/ztp/hello", &hello, None)).await;
        assert_eq!(res.status(), StatusCode::OK);
        body(res).await["state"].as_str().unwrap().to_owned()
    };
    // Adopted, and not awaiting: nothing to do.
    assert_eq!(say().await, "adopted");

    demo.store
        .put_ztp_approval(ZtpApproval::awaiting(&node, demo.api.now(), None))
        .await
        .unwrap();
    assert_eq!(say().await, "adopting");
    // The approval stays until the adoption records the device: the demo's address for it
    // isn't a simulated node, so it fails and the next hello tries again.
    let inbox = body(demo.send(demo.get("/v1/ztp/pending")).await).await;
    let job = inbox["pending"][0]["job"].clone();
    ended(&demo, &json!({ "id": job })).await;
    assert_eq!(demo.store.awaiting_nodes().await.unwrap(), [node.id]);
    assert_eq!(say().await, "adopting");
}

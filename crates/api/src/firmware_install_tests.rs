//! Installing a prepared upgrade through the router (D-124): the gates before a job starts
//! (management, the build done and of its release, an upload that fits), and the job started
//! and audited. The jobs reach no node here (the sim's addresses don't answer SSH), so they
//! stop at their login; the job itself is tested in `uniwrt-jobs`.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::NodeId;
use uniwrt_firmware::{
    Asu, ImageRequest,
    testing::{FakeAsu, HASH, sysupgrade_image},
};
use uniwrt_model::{node::NodeMode, user::AuditAction};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::{
    firmware_tests::{settled, upload},
    testing::{Demo, json as body, post, request},
};

async fn install(demo: &Demo, id: i64, what: &Value) -> (StatusCode, Value) {
    let uri = format!("/v1/firmware/nodes/{id}/install");
    let res = demo.send(post(&uri, what, Some(&demo.cookie))).await;
    (res.status(), body(res).await)
}

/// [`install`] once ASU's fake has finished the build (each build is first queued).
async fn install_built(demo: &Demo, id: i64, what: &Value) -> (StatusCode, Value) {
    for _ in 0..5 {
        let (status, answer) = install(demo, id, what).await;
        let building = answer["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("hasn't finished"));
        if !building {
            return (status, answer);
        }
    }
    panic!("the fake never finished the build");
}

async fn manage(demo: &Demo, id: i64) {
    demo.store
        .set_node_mode(NodeId::new(id), NodeMode::Managed)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn an_upload_is_installed_on_a_managed_node_by_a_job() {
    let demo = Demo::start_firmware(true, None, None).await;
    let nodes = settled(&demo).await["nodes"].as_array().unwrap().clone();
    let board = nodes[0]["board"].as_str().unwrap().to_owned();
    let id = nodes[0]["node"].as_i64().unwrap();
    let image = sysupgrade_image(&board, "25.12.7", 20_000);
    let res = demo.send(upload(&demo, "custom.bin", image)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let sha256 = body(res).await["sha256"].as_str().unwrap().to_owned();
    let what = json!({"source": "upload", "sha256": sha256});

    let (status, answer) = install(&demo, id, &what).await;
    assert_eq!(status, StatusCode::CONFLICT, "observe mode: {answer}");
    assert!(answer["detail"].as_str().unwrap().contains("observe mode"));

    manage(&demo, id).await;
    let (status, job) = install(&demo, id, &what).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["kind"], "upgrade");
    assert!(
        job["target"].as_str().unwrap().ends_with("to 25.12.7"),
        "{job}"
    );
    assert_eq!(job["node"], id);
    let audit = demo.store.audit_log(1).await.unwrap();
    assert_eq!(audit[0].action, AuditAction::Firmware);
    let detail = audit[0].detail.as_deref().unwrap();
    assert!(detail.contains("with the uploaded custom.bin"), "{detail}");

    let (status, _) = install(
        &demo,
        id,
        &json!({"source": "upload", "sha256": "0".repeat(64)}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = install(
        &demo,
        id,
        &json!({"source": "upload", "url": "https://evil"}),
    )
    .await;
    assert!(status.is_client_error(), "no URL from the client: {status}");
}

#[tokio::test]
async fn an_asu_build_must_be_done_and_of_the_release() {
    let fake = FakeAsu::start().await;
    let asu = Asu::new(fake.url(), "uniwrt/test").unwrap();
    let demo = Demo::start_firmware(true, Some(asu.clone()), None).await;
    let nodes = settled(&demo).await["nodes"].as_array().unwrap().clone();
    let older = nodes
        .iter()
        .find(|node| node["version"] == "24.10.8")
        .expect("a node on 24.10");
    let id = older["node"].as_i64().unwrap();
    manage(&demo, id).await;
    let built = |release: &str| json!({"source": "asu", "release": release, "build": HASH});

    // Not on offer: nothing is asked of ASU.
    let (status, _) = install(&demo, id, &built("24.10.7")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The fake's first answer: still building.
    let (status, answer) = install(&demo, id, &built("25.12.6")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(
        answer["detail"]
            .as_str()
            .unwrap()
            .contains("hasn't finished")
    );

    // ASU doesn't say which release it built and the names don't tell: refused.
    let (status, answer) = install_built(&demo, id, &built("25.12.6")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{answer}");
    assert!(
        answer["detail"]
            .as_str()
            .unwrap()
            .contains("isn't of 25.12.6")
    );

    // Once ASU names the release it built (`version_number`), it installs as that one only.
    asu.build(ImageRequest {
        release: "24.10.8",
        target: "mediatek/filogic",
        profile: "glinet_gl-mt6000",
        script: "",
    })
    .await
    .unwrap();
    let (status, answer) = install_built(&demo, id, &built("25.12.6")).await;
    assert_eq!(status, StatusCode::CONFLICT, "built 24.10.8: {answer}");
    asu.build(ImageRequest {
        release: "25.12.6",
        target: "mediatek/filogic",
        profile: "glinet_gl-mt6000",
        script: "",
    })
    .await
    .unwrap();
    let (status, job) = install_built(&demo, id, &built("25.12.6")).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert!(job["target"].as_str().unwrap().ends_with("to 25.12.6"));

    // A viewer can't.
    let res = demo
        .send(request(
            Method::POST,
            &format!("/v1/firmware/nodes/{id}/install"),
            Some(&built("25.12.6")),
            None,
        ))
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

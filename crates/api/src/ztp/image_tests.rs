//! Zero-touch images through the router, against the fake ASU on loopback: the catalog, a build
//! with the role's first-boot script, its progress, audits, and who may ask.

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::SystemClock;
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_firmware::{
    Asu,
    testing::{FakeAsu, HASH},
};
use uniwrt_model::user::AuditAction;
use uniwrt_sim::SimLink;
use uniwrt_store::{AuditRepo as _, Store};

use crate::{
    Api,
    testing::{Handles, PASSWORD, api_full, json as body, login, post, request, router_send},
};

struct Harness {
    _dir: tempfile::TempDir,
    fake: Option<FakeAsu>,
    store: Store,
    api: Api,
    cookie: String,
}

/// An API with an admin logged in, and the fake ASU when `asu`.
async fn harness(asu: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), clock.clone());
    let fake = if asu {
        Some(FakeAsu::start().await)
    } else {
        None
    };
    let handles = Handles {
        asu: fake
            .as_ref()
            .map(|fake| Asu::new(fake.url(), "uniwrt/test").unwrap()),
        ..Handles::default()
    };
    let api = api_full(store.clone(), Arc::new(fleet), clock, None, handles).await;
    api.create_admin("admin", PASSWORD).await.unwrap();
    let cookie = login(&api).await;
    Harness {
        _dir: dir,
        fake,
        store,
        api,
        cookie,
    }
}

impl Harness {
    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let res = router_send(
            &self.api,
            request(Method::GET, uri, None, Some(&self.cookie)),
        )
        .await;
        (res.status(), body(res).await)
    }

    async fn build(&self, body_: &Value) -> (StatusCode, Value) {
        let res = router_send(&self.api, post("/v1/ztp/builds", body_, Some(&self.cookie))).await;
        (res.status(), body(res).await)
    }
}

#[tokio::test]
async fn the_catalog_and_a_build_from_request_to_images() {
    let h = harness(true).await;
    let (status, releases) = h.get("/v1/ztp/releases").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(releases, json!({"releases": ["25.12.6", "24.10.8"]}));
    let (status, profiles) = h.get("/v1/ztp/profiles?release=25.12.6").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        profiles[2],
        json!({"id": "glinet_gl-mt6000", "target": "mediatek/filogic", "titles": ["GL.iNet GL-MT6000"]})
    );
    let (status, _) = h.get("/v1/ztp/profiles?release=19.07.10").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, started) = h
        .build(&json!({"release": "25.12.6", "target": "mediatek/filogic", "profile": "glinet_gl-mt6000", "role": "bridge"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["packages"][1], "wpad-mbedtls");
    assert_eq!(started["build"]["state"], "queued");
    let sent = &h.fake.as_ref().unwrap().builds()[0];
    let script = sent["defaults"].as_str().unwrap();
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("\nROLE='bridge'\n"));
    assert!(
        script.contains("\nURLS='https://uniwrt.lan:8443'\n"),
        "{script}"
    );

    let audit = h.store.audit_log(10).await.unwrap();
    let entry = audit.iter().find(|e| e.action == AuditAction::Ztp).unwrap();
    assert_eq!(
        entry.detail.as_deref(),
        Some(
            format!(
                "asked ASU for an image of the GL.iNet GL-MT6000 on 25.12.6 as a bridge ({HASH})"
            )
            .as_str()
        )
    );

    let (_, building) = h.get(&format!("/v1/ztp/builds/{HASH}")).await;
    assert_eq!(building["state"], "building");
    let (_, done) = h.get(&format!("/v1/ztp/builds/{HASH}")).await;
    assert_eq!(done["state"], "done");
    assert_eq!(done["images"][0]["kind"], "factory");
}

#[tokio::test]
async fn unknown_devices_refusals_and_lost_builds() {
    let h = harness(true).await;
    let (status, _) = h
        .build(&json!({"release": "25.12.6", "target": "x86/64", "profile": "nosuch"}))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, problem) = h
        .build(&json!({"release": "25.12.6", "target": "ath79/generic", "profile": "refused_one"}))
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("Unsupported profile"),
        "{problem}"
    );
    let (status, _) = h.get("/v1/ztp/builds/ffffffffffffffff").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = h.build(&json!({"release": "25.12.6"})).await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn builds_are_off_without_asu() {
    let h = harness(false).await;
    let (status, _) = h.get("/v1/ztp/releases").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

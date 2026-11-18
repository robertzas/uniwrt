//! Controller backups and their restore through the router (D-129): the passphrase, *Back up
//! now*, the download, and a restore uploaded, unlocked and applied, by an admin or during
//! first-run setup.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    body::{self, Body},
    http::{Method, StatusCode},
};
use serde_json::{Value, json};
use uniwrt_core::SystemClock;
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_model::user::AuditAction;
use uniwrt_sim::SimLink;
use uniwrt_store::{
    AuditRepo as _, Store,
    controller_backup::{ControllerBackups, Staging},
};

use crate::{
    Api,
    controller_backups::BackupConfig,
    testing::{Handles, PASSWORD, api_full, json as body, login, request, router_send},
};

struct Harness {
    dir: tempfile::TempDir,
    store: Store,
    api: Api,
    /// The admin's session; `None` before first-run setup.
    cookie: Option<String>,
    restarts: Arc<AtomicUsize>,
}

async fn harness(with_admin: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), clock.clone());
    let restarts = Arc::new(AtomicUsize::new(0));
    let handles = Handles {
        controller_backups: Some(BackupConfig {
            backups: ControllerBackups::new(dir.path().join("backups"), None).with_work_factor(4),
            staging: Staging::new(dir.path()),
            restart: Arc::new({
                let restarts = Arc::clone(&restarts);
                move || {
                    restarts.fetch_add(1, Ordering::SeqCst);
                }
            }),
        }),
        ..Handles::default()
    };
    let api = api_full(store.clone(), Arc::new(fleet), clock, None, handles).await;
    let cookie = if with_admin {
        api.create_admin("admin", PASSWORD).await.unwrap();
        Some(login(&api).await)
    } else {
        None
    };
    Harness {
        dir,
        store,
        api,
        cookie,
        restarts,
    }
}

impl Harness {
    async fn send(&self, method: Method, uri: &str, body_: Option<&Value>) -> (StatusCode, Value) {
        let res = router_send(
            &self.api,
            request(method, uri, body_, self.cookie.as_deref()),
        )
        .await;
        let status = res.status();
        let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn upload(&self, file: Vec<u8>) -> (StatusCode, Value) {
        let (parts, _) = request(
            Method::PUT,
            "/v1/controller/restore",
            None,
            self.cookie.as_deref(),
        )
        .into_parts();
        let res = router_send(
            &self.api,
            axum::http::Request::from_parts(parts, Body::from(file)),
        )
        .await;
        (res.status(), body(res).await)
    }

    async fn audits(&self) -> Vec<String> {
        self.store
            .audit_log(100)
            .await
            .unwrap()
            .into_iter()
            .filter(|entry| entry.action == AuditAction::Backup)
            .filter_map(|entry| entry.detail)
            .collect()
    }
}

#[tokio::test]
async fn an_admin_sets_the_passphrase_backs_up_and_downloads() {
    let h = harness(true).await;
    let (status, state) = h.send(Method::GET, "/v1/controller/backups", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["passphrase_set"], false);
    assert_eq!(state["backups"], json!([]));
    assert_eq!(
        (state["running"].clone(), state["at_min"].clone()),
        (json!(false), json!(120))
    );
    assert_eq!(state["restore"], json!({ "state": "none" }));

    let (status, _) = h.send(Method::POST, "/v1/controller/backups", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let short = json!({ "passphrase": "too short" });
    let (status, _) = h
        .send(
            Method::PUT,
            "/v1/controller/backups/passphrase",
            Some(&short),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let good = json!({ "passphrase": "a long enough passphrase" });
    let (status, _) = h
        .send(
            Method::PUT,
            "/v1/controller/backups/passphrase",
            Some(&good),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, backup) = h.send(Method::POST, "/v1/controller/backups", None).await;
    assert_eq!(status, StatusCode::CREATED, "{backup}");
    let name = backup["name"].as_str().unwrap().to_owned();
    let (_, state) = h.send(Method::GET, "/v1/controller/backups", None).await;
    assert_eq!(state["passphrase_set"], true);
    assert_eq!(state["backups"], json!([backup]));
    assert_eq!(state["last"]["error"], Value::Null);

    let get = request(
        Method::GET,
        &format!("/v1/controller/backups/{name}"),
        None,
        h.cookie.as_deref(),
    );
    let res = router_send(&h.api, get).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["cache-control"], "no-store");
    let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        bytes,
        std::fs::read(h.dir.path().join("backups").join(&name)).unwrap()
    );
    let (status, _) = h
        .send(Method::GET, "/v1/controller/backups/uniwrt.db", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let audits = h.audits().await;
    assert!(
        audits
            .iter()
            .any(|a| a == "set the controller backup passphrase"),
        "{audits:?}"
    );
    assert!(
        audits
            .iter()
            .any(|a| a == &format!("downloaded the controller backup {name}"))
    );
}

#[tokio::test]
async fn a_restore_is_uploaded_unlocked_and_applied_by_a_restart() {
    let h = harness(true).await;
    let good = json!({ "passphrase": "a long enough passphrase" });
    h.send(
        Method::PUT,
        "/v1/controller/backups/passphrase",
        Some(&good),
    )
    .await;
    let (_, backup) = h.send(Method::POST, "/v1/controller/backups", None).await;
    let file = std::fs::read(
        h.dir
            .path()
            .join("backups")
            .join(backup["name"].as_str().unwrap()),
    )
    .unwrap();

    let (status, _) = h
        .send(Method::POST, "/v1/controller/restore/unlock", Some(&good))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, state) = h.upload(b"not a backup".to_vec()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(state, json!({ "state": "uploaded", "size_bytes": 12 }));
    let (status, err) = h
        .send(Method::POST, "/v1/controller/restore/unlock", Some(&good))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    h.upload(file).await;
    let wrong = json!({ "passphrase": "not the passphrase" });
    let (status, err) = h
        .send(Method::POST, "/v1/controller/restore/unlock", Some(&wrong))
        .await;
    assert_eq!(
        (status, err["detail"].as_str()),
        (
            StatusCode::BAD_REQUEST,
            Some("The passphrase doesn't open this backup")
        )
    );
    let (status, state) = h
        .send(Method::POST, "/v1/controller/restore/unlock", Some(&good))
        .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(
        (
            state["state"].as_str(),
            state["manifest"]["version"].as_str()
        ),
        (Some("ready"), Some("1.2.3"))
    );
    let (_, state) = h.send(Method::GET, "/v1/controller/restore", None).await;
    assert_eq!(state["state"], "ready");

    assert_eq!(h.restarts.load(Ordering::SeqCst), 0);
    let (status, _) = h
        .send(Method::POST, "/v1/controller/restore/apply", None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(h.restarts.load(Ordering::SeqCst), 1);

    let (status, _) = h.send(Method::DELETE, "/v1/controller/restore", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, state) = h.send(Method::GET, "/v1/controller/restore", None).await;
    assert_eq!(state, json!({ "state": "none" }));
    let (status, _) = h
        .send(Method::POST, "/v1/controller/restore/apply", None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_new_host_restores_during_first_run_setup_without_a_session() {
    let h = harness(false).await;
    let (status, state) = h.send(Method::GET, "/v1/controller/restore", None).await;
    assert_eq!(
        (status, state),
        (StatusCode::OK, json!({ "state": "none" }))
    );
    let (status, _) = h.upload(b"x".to_vec()).await;
    assert_eq!(status, StatusCode::CREATED);
    // The rest stays behind a session.
    let (status, _) = h.send(Method::GET, "/v1/controller/backups", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let audits = h.audits().await;
    assert_eq!(
        audits,
        ["uploaded a controller backup to restore (1 bytes)"]
    );

    // Once the admin exists, a session is needed.
    let with_admin = harness(true).await;
    let anonymous = request(Method::GET, "/v1/controller/restore", None, None);
    assert_eq!(
        router_send(&with_admin.api, anonymous).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

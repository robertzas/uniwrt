//! The controller's own update through the router (D-123), against the fake releases on
//! loopback and a scripted [`SelfUpdate`]: the offer, installs, refusals and the setting.

use std::sync::{Arc, Mutex};

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::SystemClock;
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_firmware::{ControllerRelease, GitHub, Version, testing::FakeReleases};
use uniwrt_model::{controller::InstallProgress, user::AuditAction};
use uniwrt_sim::SimLink;
use uniwrt_store::{AuditRepo as _, Store};

use crate::{
    Api,
    controller_update::{SelfUpdate, Trials},
    testing::{Handles, PASSWORD, api_full, json as body, login, request, router_send},
};

const TARGET: &str = "x86_64-unknown-linux-musl";

/// Records installs; blocked or with rejected releases as a test sets it.
#[derive(Default)]
struct Scripted {
    blocked: Option<String>,
    rejected: Vec<Version>,
    progress: Mutex<InstallProgress>,
    installs: Mutex<Vec<Version>>,
}

impl SelfUpdate for Scripted {
    fn target(&self) -> &'static str {
        TARGET
    }

    fn blocked(&self) -> Option<String> {
        self.blocked.clone()
    }

    fn trials(&self) -> Trials {
        Trials {
            failure: None,
            rejected: self.rejected.clone(),
        }
    }

    fn progress(&self) -> InstallProgress {
        self.progress.lock().unwrap().clone()
    }

    fn install(&self, release: ControllerRelease) -> Result<(), String> {
        let mut progress = self.progress.lock().unwrap();
        if progress.busy() {
            return Err("busy".to_owned());
        }
        *progress = InstallProgress::Downloading {
            version: release.version.to_string(),
        };
        self.installs.lock().unwrap().push(release.version);
        Ok(())
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    _fake: FakeReleases,
    store: Store,
    api: Api,
    cookie: String,
}

/// An API running 1.2.3 with an admin logged in; the fake has 1.2.2 and 1.2.4 for the target
/// and 1.3.0 for another. `github` false: GitHub can't be reached.
async fn harness(updater: Option<Arc<Scripted>>, github: bool) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), clock.clone());
    let fake = FakeReleases::start().await;
    fake.publish("1.2.2", TARGET, b"older");
    fake.publish("1.2.4", TARGET, b"newer");
    fake.publish("1.3.0", "aarch64-unknown-linux-musl", b"other target");
    let base = if github {
        fake.url()
    } else {
        "http://127.0.0.1:9"
    };
    let handles = Handles {
        github: Some(GitHub::new(base, "uniwrt/test").unwrap()),
        self_update: updater.map(|updater| updater as Arc<dyn SelfUpdate>),
        ..Handles::default()
    };
    let api = api_full(store.clone(), Arc::new(fleet), clock, None, handles).await;
    api.create_admin("admin", PASSWORD).await.unwrap();
    let cookie = login(&api).await;
    Harness {
        _dir: dir,
        _fake: fake,
        store,
        api,
        cookie,
    }
}

impl Harness {
    async fn send(&self, method: Method, uri: &str, body_: Option<&Value>) -> (StatusCode, Value) {
        let res = router_send(&self.api, request(method, uri, body_, Some(&self.cookie))).await;
        (res.status(), body(res).await)
    }

    async fn status(&self) -> Value {
        let (status, update) = self.send(Method::GET, "/v1/controller/update", None).await;
        assert_eq!(status, StatusCode::OK, "{update}");
        update
    }

    async fn install(&self, version: &str) -> (StatusCode, Value) {
        let body_ = json!({ "version": version });
        self.send(Method::POST, "/v1/controller/update", Some(&body_))
            .await
    }

    async fn audits(&self) -> Vec<String> {
        self.store
            .audit_log(50)
            .await
            .unwrap()
            .into_iter()
            .filter(|entry| entry.action == AuditAction::Controller)
            .filter_map(|entry| entry.detail)
            .collect()
    }
}

#[tokio::test]
async fn the_newest_newer_release_is_offered_and_installed_once() {
    let updater = Arc::new(Scripted::default());
    let h = harness(Some(Arc::clone(&updater)), true).await;
    let update = h.status().await;
    assert_eq!(update["current"], "1.2.3");
    assert_eq!(update["offer"]["version"], "1.2.4");
    assert!(update["offer"]["size_bytes"].as_u64().unwrap() > 0);
    assert_eq!(update["note"], Value::Null);
    assert_eq!(update["blocked"], Value::Null);
    assert_eq!(update["progress"], json!({"state": "idle"}));
    assert_eq!(update["settings"], json!({"auto": false, "at_min": 150}));

    // Only the release on offer.
    let (status, refused) = h.install("1.2.2").await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let (status, started) = h.install("1.2.4").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    assert_eq!(
        started["progress"],
        json!({"state": "downloading", "version": "1.2.4"})
    );
    assert_eq!(*updater.installs.lock().unwrap(), [Version(1, 2, 4)]);
    assert_eq!(
        h.audits().await,
        ["installing the controller's release 1.2.4 (running 1.2.3)"]
    );

    // A second install waits for the first.
    let (status, busy) = h.install("1.2.4").await;
    assert_eq!(status, StatusCode::CONFLICT, "{busy}");
    assert_eq!(updater.installs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rejected_releases_blocked_controllers_and_github_down_install_nothing() {
    let rejected = Arc::new(Scripted {
        rejected: vec![Version(1, 2, 4)],
        ..Scripted::default()
    });
    let h = harness(Some(Arc::clone(&rejected)), true).await;
    assert_eq!(h.status().await["offer"], Value::Null);
    let (status, none) = h.install("1.2.4").await;
    assert_eq!(status, StatusCode::CONFLICT, "{none}");
    assert!(rejected.installs.lock().unwrap().is_empty());

    let blocked = Arc::new(Scripted {
        blocked: Some("Demo mode doesn't update itself".to_owned()),
        ..Scripted::default()
    });
    let h = harness(Some(Arc::clone(&blocked)), true).await;
    let update = h.status().await;
    assert_eq!(update["offer"]["version"], "1.2.4");
    assert_eq!(update["blocked"], "Demo mode doesn't update itself");
    let (status, refused) = h.install("1.2.4").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["detail"], "Demo mode doesn't update itself");
    assert!(blocked.installs.lock().unwrap().is_empty());
    assert_eq!(h.audits().await, Vec::<String>::new());

    let offline = Arc::new(Scripted::default());
    let h = harness(Some(Arc::clone(&offline)), false).await;
    let update = h.status().await;
    assert_eq!(update["offer"], Value::Null);
    assert!(
        update["note"].as_str().unwrap().contains("GitHub"),
        "{update}"
    );
    let (status, _) = h.install("1.2.4").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn the_automatic_install_is_set_by_admins_and_audited() {
    let h = harness(Some(Arc::new(Scripted::default())), true).await;
    let uri = "/v1/controller/update/settings";
    let (status, _) = h
        .send(
            Method::PUT,
            uri,
            Some(&json!({"auto": true, "at_min": 1440})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, saved) = h
        .send(
            Method::PUT,
            uri,
            Some(&json!({"auto": true, "at_min": 120})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        h.status().await["settings"],
        json!({"auto": true, "at_min": 120})
    );
    assert_eq!(
        h.audits().await,
        ["turned the controller's automatic install on, nightly at 02:00"]
    );
}

#[tokio::test]
async fn without_an_updater_the_routes_are_unavailable() {
    let h = harness(None, true).await;
    let (status, _) = h.send(Method::GET, "/v1/controller/update", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

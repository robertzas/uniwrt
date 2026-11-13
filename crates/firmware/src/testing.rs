//! A fake ASU and download server on loopback (HTTP), for tests: two releases, three devices
//! (one with the basic wpad, one without Wi-Fi, one ASU refuses), and builds that are queued,
//! then building, then done.

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};

/// The hash of every build the fake makes.
pub const HASH: &str = "0123456789abcdef0123456789abcdef";
/// The SHA-256 of every image.
pub const SHA256: &str = "a25a05977027deae000b10b025b75ed3e9c12a4b7d42ae57e8d5a8a019493658";

/// The running fake. Dropping it stops it.
pub struct FakeAsu {
    url: String,
    state: Arc<Mutex<Seen>>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
struct Seen {
    builds: Vec<Value>,
    polls: usize,
}

type Shared = Arc<Mutex<Seen>>;

impl FakeAsu {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let state = Shared::default();
        let upstream = url.clone();
        let app = Router::new()
            .route(
                "/json/v1/overview.json",
                get(move || async move {
                    Json(json!({"latest": ["24.10.8", "25.12.6", "SNAPSHOT"], "upstream_url": upstream, "branches": {}}))
                }),
            )
            .route("/releases/{release}/.overview.json", get(profiles))
            .route(
                "/json/v1/releases/{release}/targets/{target}/{subtarget}/{file}",
                get(profile),
            )
            .route("/api/v1/build", post(build))
            .route("/api/v1/build/{hash}", get(poll))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { url, state, task }
    }

    /// Its URL, ASU's and the download server's.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The build requests it got, as JSON.
    pub fn builds(&self) -> Vec<Value> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .builds
            .clone()
    }
}

impl Drop for FakeAsu {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn profiles(Path(release): Path<String>) -> (StatusCode, Json<Value>) {
    if release != "25.12.6" && release != "24.10.8" {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    let profiles = json!({"release": release, "profiles": [
        {"id": "glinet_gl-mt6000", "target": "mediatek/filogic", "titles": [{"vendor": "GL.iNet", "model": "GL-MT6000"}]},
        {"id": "generic", "target": "x86/64", "titles": [{"title": "Generic x86/64"}]},
        {"id": "refused_one", "target": "ath79/generic", "titles": [{"vendor": "Acme", "model": "Refused"}]}
    ]});
    (StatusCode::OK, Json(profiles))
}

async fn profile(
    Path((_release, _target, _subtarget, file)): Path<(String, String, String, String)>,
) -> (StatusCode, Json<Value>) {
    let defaults = json!(["dnsmasq", "firewall4", "wpad-basic-mbedtls"]);
    match file.as_str() {
        "glinet_gl-mt6000.json" | "refused_one.json" => (
            StatusCode::OK,
            Json(json!({"default_packages": defaults, "device_packages": ["kmod-mt7915e"]})),
        ),
        "generic.json" => (
            StatusCode::OK,
            Json(json!({"default_packages": ["dnsmasq", "e2fsprogs"], "device_packages": []})),
        ),
        _ => (StatusCode::NOT_FOUND, Json(json!({}))),
    }
}

async fn build(
    State(state): State<Shared>,
    Json(request): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let refused = request["profile"] == "refused_one";
    let mut seen = state.lock().unwrap_or_else(PoisonError::into_inner);
    seen.builds.push(request);
    seen.polls = 0;
    if refused {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": 400, "detail": "Unsupported profile: refused_one"})),
        );
    }
    (
        StatusCode::ACCEPTED,
        Json(json!({"status": 202, "detail": "queued", "queue_position": 2, "request_hash": HASH})),
    )
}

async fn poll(State(state): State<Shared>, Path(hash): Path<String>) -> (StatusCode, Json<Value>) {
    if hash != HASH {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status": 404, "detail": "could not find provided request hash"})),
        );
    }
    let polls = {
        let mut seen = state.lock().unwrap_or_else(PoisonError::into_inner);
        seen.polls += 1;
        seen.polls
    };
    if polls == 1 {
        return (
            StatusCode::ACCEPTED,
            Json(
                json!({"status": 202, "detail": "started", "imagebuilder_status": "building_image", "request_hash": HASH}),
            ),
        );
    }
    let image = |name: &str, kind: &str| json!({"name": name, "type": kind, "filesystem": "squashfs", "size": 10_598_720, "sha256": SHA256});
    (
        StatusCode::OK,
        Json(
            json!({"status": 200, "detail": "done", "request_hash": HASH, "bin_dir": HASH, "images": [
                image("openwrt-squashfs-sysupgrade.bin", "sysupgrade"),
                image("openwrt-squashfs-factory.bin", "factory")
            ]}),
        ),
    )
}

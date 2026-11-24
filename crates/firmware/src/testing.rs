//! A fake ASU and download server on loopback (HTTP), for tests: two releases, three devices
//! (one with the basic wpad, one without Wi-Fi, one ASU refuses), and builds that are queued,
//! then building, then done. For upgrades: the branches 25.12 and 24.10 (24.10.7 and 24.10.8,
//! with opkg renamed apk-mbedtls between them), `mediatek/filogic`'s `profiles.json` in each
//! (the GL-MT6000 and GL-MT3000), and
//! package indexes with LuCI and the mesh's packages. [`FakeGitHub`] is GitHub's releases API.

mod controller;
mod github;

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};

pub use controller::{FakeReleases, tarball};
pub use github::{FakeGitHub, PRIVATE_ASSET, TOKEN};

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
                    Json(json!({"latest": ["24.10.8", "25.12.6", "SNAPSHOT"], "upstream_url": upstream, "branches": {
                        "SNAPSHOT": {"snapshot": true, "versions": ["SNAPSHOT"]},
                        "25.12": {"versions": ["25.12.6", "25.12.5"], "branch_off_rev": 32295, "package_changes": [
                            {"source": "opkg", "target": "apk-mbedtls", "revision": 28056}
                        ]},
                        "24.10": {"versions": ["24.10.8", "24.10.7"], "branch_off_rev": 27990, "package_changes": []}
                    }}))
                }),
            )
            .route("/releases/{release}/.overview.json", get(profiles))
            .route(
                "/releases/{release}/targets/{target}/{subtarget}/profiles.json",
                get(target_profiles),
            )
            .route("/json/v1/releases/{release}/packages/{file}", get(arch_index))
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

/// The vermagic of every release's kernel: its kernel modules are in
/// `kmods/<version>-1-<this>`.
pub const KERNEL_HASH: &str = "b4125196ecec75391093e1e2edeba490";

/// The sysupgrade image's size in every release.
pub const SYSUPGRADE_BYTES: u64 = 10_598_720;

/// A sysupgrade image of `size` bytes or so for `board` (in `mediatek/filogic`) and OpenWrt
/// `version`, with its metadata and a signature appended as `fwtool` does.
pub fn sysupgrade_image(board: &str, version: &str, size: usize) -> Vec<u8> {
    let json = json!({"metadata_version": "1.1", "supported_devices": [board], "version": {
        "dist": "OpenWrt", "version": version, "revision": "r1-test", "target": "mediatek/filogic",
        "board": board.replace(',', "_")}});
    let mut metadata = vec![0; 8];
    metadata.extend_from_slice(json.to_string().as_bytes());
    let mut image: Vec<u8> = (0..size)
        .map(|i| u8::try_from(i % 251).unwrap_or(0))
        .collect();
    for (kind, data) in [
        (1_u8, metadata.as_slice()),
        (0, b"# fake certificate".as_slice()),
    ] {
        image.extend_from_slice(data);
        image.extend_from_slice(b"FWx0");
        image.extend_from_slice(&[0; 4]);
        image.extend_from_slice(&[kind, 0, 0, 0]);
        let len = u32::try_from(data.len() + 16).expect("a small chunk");
        image.extend_from_slice(&len.to_be_bytes());
    }
    image
}

async fn target_profiles(
    Path((release, target, subtarget)): Path<(String, String, String)>,
) -> (StatusCode, Json<Value>) {
    let manager = match release.as_str() {
        "25.12.6" | "25.12.5" => "apk-mbedtls",
        "24.10.8" | "24.10.7" => "opkg",
        _ => return (StatusCode::NOT_FOUND, Json(json!({}))),
    };
    if (target.as_str(), subtarget.as_str()) != ("mediatek", "filogic") {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    (
        StatusCode::OK,
        Json(json!({
            "arch_packages": "aarch64_cortex-a53",
            "default_packages": [manager, "dnsmasq", "ppp", "wpad-basic-mbedtls"],
            "linux_kernel": {"version": if manager == "opkg" { "6.6.119" } else { "6.12.112" }, "release": "1", "vermagic": KERNEL_HASH},
            "profiles": {
                "glinet_gl-mt6000": {
                    "device_packages": ["kmod-mt7915e"],
                    "supported_devices": ["glinet,gl-mt6000"],
                    "images": [
                        {"type": "factory", "name": format!("openwrt-{release}-mediatek-filogic-glinet_gl-mt6000-squashfs-factory.bin"), "sha256": SHA256},
                        {"type": "sysupgrade", "size": SYSUPGRADE_BYTES, "filesystem": "squashfs", "sha256": SHA256,
                         "name": format!("openwrt-{release}-mediatek-filogic-glinet_gl-mt6000-squashfs-sysupgrade.bin")}
                    ]
                },
                "glinet_gl-mt3000": {
                    "device_packages": ["kmod-mt7915e"],
                    "supported_devices": ["glinet,gl-mt3000"],
                    "images": [{"type": "sysupgrade", "size": SYSUPGRADE_BYTES}]
                }
            }
        })),
    )
}

async fn arch_index(Path((_release, file)): Path<(String, String)>) -> (StatusCode, Json<Value>) {
    if file != "aarch64_cortex-a53-index.json" {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    (
        StatusCode::OK,
        Json(json!({"luci": "26.1", "wpad-mbedtls": "1", "batctl-default": "1", "dnsmasq": "2"})),
    )
}

async fn profile(
    Path((_release, _target, _subtarget, file)): Path<(String, String, String, String)>,
) -> (StatusCode, Json<Value>) {
    let defaults = json!(["dnsmasq", "firewall4", "wpad-basic-mbedtls"]);
    match file.as_str() {
        "index.json" => (
            StatusCode::OK,
            Json(
                json!({"architecture": "aarch64_cortex-a53", "packages": {"kmod-batman-adv": "1", "kmod-mt7915e": "1"}}),
            ),
        ),
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
    let (polls, version) = {
        let mut seen = state.lock().unwrap_or_else(PoisonError::into_inner);
        seen.polls += 1;
        let version = seen
            .builds
            .last()
            .and_then(|build| build["version"].as_str())
            .map(str::to_owned);
        (seen.polls, version)
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
            json!({"status": 200, "detail": "done", "request_hash": HASH, "bin_dir": HASH, "version_number": version, "images": [
                image("openwrt-squashfs-sysupgrade.bin", "sysupgrade"),
                image("openwrt-squashfs-factory.bin", "factory")
            ]}),
        ),
    )
}

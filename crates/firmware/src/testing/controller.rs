//! A fake of uniwrt's own releases on GitHub (D-123), for tests: `robertzas/uniwrt`'s releases
//! (each a tag `vX.Y.Z` with a tarball for one target), the tarballs behind a redirect as
//! GitHub serves them, and an attestation of each tarball (`{"fake": "<sha256>"}`: tests
//! verify with a closure that accepts it).

use std::{
    io::Write as _,
    sync::{Arc, Mutex, PoisonError},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
    routing::get,
};
use data_encoding::HEXLOWER;
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};

/// A gzipped tarball of `files` (name, contents), with GNU's headers as CI's `tar` writes them.
pub fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
    const BLOCK: usize = 512;
    let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
    for (name, data) in files {
        let mut header = [0_u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000755\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[156] = b'0';
        header[257..265].copy_from_slice(b"ustar  \0");
        header[148..156].copy_from_slice(b"        ");
        let sum: u64 = header.iter().map(|b| u64::from(*b)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let pad = data.len().div_ceil(BLOCK) * BLOCK - data.len();
        for chunk in [&header[..], data, &vec![0; pad]] {
            gz.write_all(chunk).expect("in memory");
        }
    }
    gz.write_all(&[0; 2 * BLOCK]).expect("in memory");
    gz.finish().expect("in memory")
}

/// One release: its version, the tarball's name and bytes, and whether GitHub lists the right
/// digest for it.
struct Served {
    version: String,
    name: String,
    bytes: Vec<u8>,
    sha256: String,
}

type Shared = Arc<Mutex<Vec<Served>>>;

/// The running fake. Dropping it stops it.
pub struct FakeReleases {
    url: String,
    releases: Shared,
    task: tokio::task::JoinHandle<()>,
}

impl FakeReleases {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let releases = Shared::default();
        let app = Router::new()
            .route("/repos/robertzas/uniwrt/releases", get(list))
            .route(
                "/repos/robertzas/uniwrt/attestations/{digest}",
                get(attestations),
            )
            .route("/download/{name}", get(redirect))
            .route("/blob/{name}", get(blob))
            .with_state((url.clone(), Arc::clone(&releases)));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url,
            releases,
            task,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Publishes `version` with a tarball for `target` holding `binary` as uniwrt's.
    pub fn publish(&self, version: &str, target: &str, binary: &[u8]) {
        let dir = format!("uniwrt-{version}-{target}");
        let bytes = tarball(&[
            (&format!("{dir}/README.md"), b"uniwrt"),
            (&format!("{dir}/uniwrt"), binary),
        ]);
        let sha256 = HEXLOWER.encode(ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref());
        self.lock().push(Served {
            version: version.to_owned(),
            name: format!("uniwrt-{version}-{target}.tar.gz"),
            bytes,
            sha256,
        });
    }

    /// Lists `version`'s tarball with another SHA-256 than its bytes have.
    pub fn misdigest(&self, version: &str) {
        for served in self.lock().iter_mut().filter(|s| s.version == version) {
            served.sha256 = "0".repeat(64);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Served>> {
        self.releases.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for FakeReleases {
    fn drop(&mut self) {
        self.task.abort();
    }
}

type AppState = State<(String, Shared)>;

async fn list(State((base, releases)): AppState) -> Json<Value> {
    let releases = releases.lock().unwrap_or_else(PoisonError::into_inner);
    Json(Value::Array(
        releases
            .iter()
            .map(|served| {
                json!({
                    "tag_name": format!("v{}", served.version),
                    "name": format!("v{}", served.version),
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2026-11-16T12:00:00Z",
                    "html_url": format!("{base}/robertzas/uniwrt/releases/tag/v{}", served.version),
                    "assets": [{
                        "name": served.name,
                        "size": served.bytes.len(),
                        "state": "uploaded",
                        "digest": format!("sha256:{}", served.sha256),
                        "browser_download_url": format!("{base}/download/{}", served.name),
                    }],
                })
            })
            .collect(),
    ))
}

async fn attestations(State((_, releases)): AppState, Path(digest): Path<String>) -> Response {
    let releases = releases.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let known = releases.iter().any(|served| {
        HEXLOWER.encode(ring::digest::digest(&ring::digest::SHA256, &served.bytes).as_ref()) == hex
    });
    if known {
        Json(json!({"attestations": [{"bundle": {"fake": hex}}]})).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))).into_response()
    }
}

async fn redirect(State((base, _)): AppState, Path(name): Path<String>) -> Response {
    (
        StatusCode::FOUND,
        [(LOCATION, format!("{base}/blob/{name}"))],
    )
        .into_response()
}

async fn blob(State((_, releases)): AppState, Path(name): Path<String>) -> Response {
    let releases = releases.lock().unwrap_or_else(PoisonError::into_inner);
    match releases.iter().find(|served| served.name == name) {
        Some(served) => served.bytes.clone().into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

//! A fake GitHub releases API on loopback (HTTP), for tests. Repos:
//!
//! - `owner/images`: `v3` (a prerelease, the newest: the GL-MT6000), `v2` (the GL-MT6000 and
//!   GL-MT3000 sysupgrade images, a factory image, checksums, an x86 image), `v1` (the
//!   GL-MT6000 without a SHA-256), and a draft.
//! - `owner/private`: one release, served only with the token [`TOKEN`]; else 404.
//! - `owner/limited`: the rate limit spent (403).

use std::sync::{Arc, Mutex, PoisonError};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};

use super::SHA256;

/// The token `owner/private` wants.
pub const TOKEN: &str = "github_pat_test";

/// The running fake. Dropping it stops it.
pub struct FakeGitHub {
    url: String,
    seen: Seen,
    task: tokio::task::JoinHandle<()>,
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

impl FakeGitHub {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("loopback");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let seen = Seen::default();
        let app = Router::new()
            .route("/repos/{owner}/{repo}/releases", get(releases))
            .with_state((url.clone(), Arc::clone(&seen)));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { url, seen, task }
    }

    /// Its URL: GitHub's API's.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The repos asked for, each with the `Authorization` header sent.
    pub fn requests(&self) -> Vec<(String, Option<String>)> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for FakeGitHub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn asset(base: &str, tag: &str, name: &str, hashed: bool) -> Value {
    json!({
        "name": name,
        "size": 9_437_184,
        "state": "uploaded",
        "digest": if hashed { Value::String(format!("sha256:{SHA256}")) } else { Value::Null },
        "url": format!("{base}/api/assets/{name}"),
        "browser_download_url": format!("{base}/owner/images/releases/download/{tag}/{name}"),
    })
}

fn release(base: &str, tag: &str, published: Option<&str>, assets: &[Value]) -> Value {
    json!({
        "tag_name": tag,
        "name": format!("Build {tag}"),
        "draft": published.is_none(),
        "prerelease": tag == "v3",
        "published_at": published,
        "html_url": format!("{base}/owner/images/releases/tag/{tag}"),
        "assets": assets,
    })
}

const MT6000: &str = "openwrt-25.12.6-mediatek-filogic-glinet_gl-mt6000-squashfs-sysupgrade.bin";

async fn releases(
    State((base, seen)): State<(String, Seen)>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    seen.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((format!("{owner}/{repo}"), token.clone()));
    let b = base.as_str();
    match (owner.as_str(), repo.as_str()) {
        ("owner", "images") => Json(json!([
            release(b, "draft", None, &[asset(b, "draft", MT6000, true)]),
            release(
                b,
                "v3",
                Some("2026-11-10T08:00:00Z"),
                &[asset(b, "v3", MT6000, true)]
            ),
            release(
                b,
                "v2",
                Some("2026-11-01T08:00:00Z"),
                &[
                    asset(b, "v2", MT6000, true),
                    asset(
                        b,
                        "v2",
                        "openwrt-25.12.6-mediatek-filogic-glinet_gl-mt3000-squashfs-sysupgrade.bin",
                        true
                    ),
                    asset(
                        b,
                        "v2",
                        "openwrt-25.12.6-mediatek-filogic-glinet_gl-mt6000-squashfs-factory.bin",
                        true
                    ),
                    asset(
                        b,
                        "v2",
                        "openwrt-25.12.6-x86-64-generic-squashfs-combined-efi.img.gz",
                        true
                    ),
                    asset(b, "v2", "sha256sums", true),
                ]
            ),
            release(
                b,
                "v1",
                Some("2026-10-01T08:00:00Z"),
                &[asset(b, "v1", MT6000, false)]
            ),
        ]))
        .into_response(),
        ("owner", "private") if token.as_deref() == Some(&format!("Bearer {TOKEN}")) => {
            Json(json!([release(
                b,
                "p1",
                Some("2026-11-02T08:00:00Z"),
                &[asset(b, "p1", MT6000, true)]
            )]))
            .into_response()
        }
        ("owner", "limited") => (
            StatusCode::FORBIDDEN,
            [
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1794000000"),
            ],
            Json(json!({"message": "API rate limit exceeded"})),
        )
            .into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"}))).into_response(),
    }
}

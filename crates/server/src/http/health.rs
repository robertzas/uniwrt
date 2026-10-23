//! `GET /api/v1/health`: liveness plus the build identity.

use axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse as _, Response},
};
use serde::Serialize;
use ts_rs::TS;

use crate::build_info;

/// The controller's liveness and build identity.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct Health {
    /// `ok` while the controller serves requests.
    pub(crate) status: &'static str,
    /// The release version; local builds report `0.0.0-dev`.
    pub(crate) version: &'static str,
    /// The commit the binary was built from, or `unknown`.
    pub(crate) commit: &'static str,
    /// When the binary was built, in RFC 3339 UTC.
    pub(crate) built_at: &'static str,
}

pub(crate) async fn get() -> Response {
    let health = Health {
        status: "ok",
        version: build_info::VERSION,
        commit: build_info::COMMIT,
        built_at: build_info::BUILT_AT,
    };
    (
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(health),
    )
        .into_response()
}

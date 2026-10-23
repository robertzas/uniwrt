//! `GET /api/v1/health`: liveness plus the build identity.

use axum::{
    Json,
    http::{HeaderValue, header},
    response::{IntoResponse as _, Response},
};
use serde::Serialize;

use crate::build_info;

#[derive(Debug, Serialize)]
pub(crate) struct Health {
    pub(crate) status: &'static str,
    pub(crate) version: &'static str,
    pub(crate) commit: &'static str,
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

//! `GET /api/v1/health`: liveness plus the build identity.

use axum::{Json, extract::State};
use serde::Serialize;
use ts_rs::TS;

use crate::Api;

/// The build identity, from the server's build script.
#[derive(Clone, Copy, Debug)]
pub struct BuildInfo {
    /// The release version; local builds report `0.0.0-dev`.
    pub version: &'static str,
    /// The commit the binary was built from, or `unknown`.
    pub commit: &'static str,
    /// When the binary was built, in RFC 3339 UTC.
    pub built_at: &'static str,
}

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

pub(crate) async fn get(State(api): State<Api>) -> Json<Health> {
    let build = api.build();
    Json(Health {
        status: "ok",
        version: build.version,
        commit: build.commit,
        built_at: build.built_at,
    })
}

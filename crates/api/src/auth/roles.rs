//! Roles at the router (features/security.md, Users; threat-model.md §1): every API route
//! needs a session unless [`PUBLIC`] names it; a viewer's session reads, and every
//! state-changing route is for admins unless [`VIEWER_WRITES`] names it; the reads in
//! [`ADMIN_READS`] (secrets, archives, terminals, jobs' inputs) are for admins too. Handlers keep
//! their own checks: this layer makes "admins only" the default for a route that forgets one.
//!
//! It runs inside the CSRF check and before the handler's extractors, so a refusal doesn't
//! depend on the body, and hands the session's [`CurrentUser`](session::CurrentUser) on in the request's extensions,
//! so the handler doesn't look it up again. The controller's restore is open without a session
//! while setup is (a fresh controller restoring its backup, D-129).

use axum::{
    extract::{MatchedPath, Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};
use uniwrt_model::user::Role;

use super::{session, setup::SetupState};
use crate::{Api, ApiError};

/// Who may call a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Anyone; the handler checks what it needs.
    Public,
    /// Any session.
    Viewer,
    /// An admin's session.
    Admin,
    /// An admin's session, or none while setup is open.
    AdminOrSetup,
}

/// Routes without a session: `(method, path)` as the router matches them.
pub(crate) const PUBLIC: &[(&str, &str)] = &[
    ("GET", "/v1/health"),
    ("GET", "/v1/tls"),
    ("GET", "/v1/tls/ca.crt"),
    ("GET", "/v1/auth"),
    ("POST", "/v1/auth/setup"),
    ("POST", "/v1/auth/login"),
    ("POST", "/v1/auth/logout"),
    ("POST", "/v1/ztp/hello"),
];

/// State-changing routes a viewer may call: shared view preferences (saved log searches, the
/// topology's pinned layout), which change no node and reveal nothing.
pub(crate) const VIEWER_WRITES: &[(&str, &str)] = &[
    ("POST", "/v1/logs/searches"),
    ("DELETE", "/v1/logs/searches/{id}"),
    ("DELETE", "/v1/topology/layout"),
    ("PUT", "/v1/topology/layout/{vertex}"),
    ("DELETE", "/v1/topology/layout/{vertex}"),
];

/// Reads only admins may make: secrets and what holds them (archives, bundles, captures,
/// scripts with tokens), a node's shell, and what a viewer couldn't act on anyway.
pub(crate) const ADMIN_READS: &[&str] = &[
    "/v1/apply/plan",
    "/v1/bootstrap/gateway",
    "/v1/controller/backups/{name}",
    "/v1/nodes/{id}/backups/{backup}/archive",
    "/v1/nodes/{id}/diag/capture",
    "/v1/nodes/{id}/terminal",
    "/v1/support/bundle",
    "/v1/ztp/builds/{hash}",
    "/v1/ztp/one-liners/{token}",
    "/v1/ztp/pending",
    "/v1/ztp/profiles",
    "/v1/ztp/releases",
    "/v1/ztp/script",
];

/// The restore's routes: [`Access::AdminOrSetup`].
const RESTORE: &str = "/v1/controller/restore";

/// What the router refuses a viewer.
pub(crate) const REFUSAL: &str = "Only admins can do this";

/// Who may call `method` on `path`, a route's pattern under `/api`.
pub(crate) fn access(method: &Method, path: &str) -> Access {
    let method = method.as_str();
    if PUBLIC.contains(&(method, path)) {
        return Access::Public;
    }
    if path == RESTORE || path.starts_with("/v1/controller/restore/") {
        return Access::AdminOrSetup;
    }
    let reads = matches!(method, "GET" | "HEAD" | "OPTIONS" | "CONNECT");
    if reads && !ADMIN_READS.contains(&path) || !reads && VIEWER_WRITES.contains(&(method, path)) {
        Access::Viewer
    } else {
        Access::Admin
    }
}

/// The layer: refuses requests their session's role doesn't allow, before the handler runs.
pub(crate) async fn check(
    State(api): State<Api>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // No pattern: no route matched, and the fallback answers.
    let Some(path) = request.extensions().get::<MatchedPath>() else {
        return Ok(next.run(request).await);
    };
    let path = path.as_str();
    let access = access(request.method(), path.strip_prefix("/api").unwrap_or(path));
    let needs_session = match access {
        Access::Public => false,
        Access::AdminOrSetup => api.setup().state().0 != SetupState::Open,
        Access::Viewer | Access::Admin => true,
    };
    if !needs_session {
        return Ok(next.run(request).await);
    }
    let user = match session::presented(request.headers()) {
        Some(token) => session::current(&api, token).await?,
        None => None,
    }
    .ok_or_else(|| ApiError::Unauthorized("Log in first".to_owned()))?;
    if access != Access::Viewer && user.user.role != Role::Admin {
        return Err(ApiError::Forbidden(REFUSAL.to_owned()));
    }
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

#[cfg(test)]
#[path = "roles_tests.rs"]
mod tests;

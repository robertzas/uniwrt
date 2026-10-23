//! API errors as RFC 9457 problem details (`application/problem+json`).

use axum::{
    Json,
    extract::OriginalUri,
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
}

pub(crate) fn response(status: StatusCode, detail: String) -> Response {
    let problem = Problem {
        kind: "about:blank",
        title: status.canonical_reason().unwrap_or("Error"),
        status: status.as_u16(),
        detail,
    };
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        ),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
    ];
    (status, headers, Json(problem)).into_response()
}

/// An unknown API path: a JSON 404, never the SPA page.
pub(crate) fn no_route(path: &str) -> Response {
    response(
        StatusCode::NOT_FOUND,
        format!("No API route matches {path}"),
    )
}

/// Fallback for the `/api` router.
pub(crate) async fn not_found(OriginalUri(uri): OriginalUri) -> Response {
    no_route(uri.path())
}

/// A known API path with an unsupported method. Axum adds the `Allow` header.
pub(crate) async fn method_not_allowed(method: Method, OriginalUri(uri): OriginalUri) -> Response {
    response(
        StatusCode::METHOD_NOT_ALLOWED,
        format!("{method} is not supported on {}", uri.path()),
    )
}

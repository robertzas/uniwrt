//! API errors as RFC 9457 problem details (`application/problem+json`): [`ApiError`], which
//! handlers return, and the fallbacks for unknown routes and methods.

use std::time::Duration;

use axum::{
    Json,
    extract::{OriginalUri, rejection::JsonRejection},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::error;

#[derive(Debug, Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
}

/// A problem+json response with `about:blank` as its type, so the title is the status's.
pub fn response(status: StatusCode, detail: String) -> Response {
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
pub fn no_route(path: &str) -> Response {
    response(
        StatusCode::NOT_FOUND,
        format!("No API route matches {path}"),
    )
}

/// Fallback for the API router.
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

/// What a handler can fail with; each becomes a problem response.
#[derive(Debug)]
pub enum ApiError {
    /// The request is malformed or its content invalid (400).
    BadRequest(String),
    /// No valid session (401); the response also clears the session cookie.
    Unauthorized(String),
    /// Authenticated or not, the request is refused (403).
    Forbidden(String),
    /// The request conflicts with the current state (409).
    Conflict(String),
    /// Too many attempts; retry after this long (429).
    TooManyRequests(Duration),
    /// A failure on our side (500); the detail is logged, never sent.
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(detail) => response(StatusCode::BAD_REQUEST, detail),
            Self::Unauthorized(detail) => {
                let mut response = response(StatusCode::UNAUTHORIZED, detail);
                response
                    .headers_mut()
                    .insert(header::SET_COOKIE, crate::auth::session::clear_cookie());
                response
            }
            Self::Forbidden(detail) => response(StatusCode::FORBIDDEN, detail),
            Self::Conflict(detail) => response(StatusCode::CONFLICT, detail),
            Self::TooManyRequests(wait) => {
                let secs = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
                let mut response = response(
                    StatusCode::TOO_MANY_REQUESTS,
                    format!("Too many attempts; try again in {secs} s"),
                );
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from(secs));
                response
            }
            Self::Internal(detail) => {
                error!("request failed: {detail}");
                response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "The controller failed to handle the request; its log has the details"
                        .to_owned(),
                )
            }
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(detail)
            | Self::Unauthorized(detail)
            | Self::Forbidden(detail)
            | Self::Conflict(detail)
            | Self::Internal(detail) => f.write_str(detail),
            Self::TooManyRequests(wait) => write!(f, "too many attempts; retry in {wait:?}"),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<uniwrt_store::Error> for ApiError {
    fn from(err: uniwrt_store::Error) -> Self {
        Self::Internal(format!("{:#}", error_chain(&err)))
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

/// An error with its sources, `outer: inner: …`.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        text.push_str(": ");
        text.push_str(&err.to_string());
        source = err.source();
    }
    text
}

pub type ApiResult<T> = Result<T, ApiError>;

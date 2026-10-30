//! Security headers on every response (features/security.md): a strict CSP, no framing, no MIME
//! sniffing, no referrers and same-origin isolation. A handler may set its own value first.
//!
//! [`csrf`] guards the API's state-changing requests: they must carry `X-Uniwrt-Csrf: 1` (a
//! custom header, which a cross-site form cannot send and a cross-site script cannot without
//! a CORS preflight that we never grant), and an `Origin`, when sent, must be our own. Session
//! cookies add `SameSite=Strict` on top.

use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue, Method, header},
    middleware::Next,
    response::Response,
};

use crate::{ApiError, Client};

/// Same-origin everything; `data:` images only for small inlined assets. No inline scripts or
/// styles: the UI sets dynamic geometry through the CSSOM, which CSP allows.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; img-src 'self' data:; object-src 'none'; \
     base-uri 'none'; frame-ancestors 'none'; form-action 'self'";

const HEADERS: [(HeaderName, &str); 6] = [
    (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (header::X_FRAME_OPTIONS, "DENY"),
    (header::REFERRER_POLICY, "no-referrer"),
    (
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    ),
    (
        HeaderName::from_static("cross-origin-resource-policy"),
        "same-origin",
    ),
];

/// The header state-changing API requests must carry, with the value `1`.
pub const CSRF_HEADER: &str = "x-uniwrt-csrf";

/// Adds the security headers to a response (`middleware::map_response`, which takes an async
/// function).
#[allow(clippy::unused_async)]
pub async fn headers(mut response: Response) -> Response {
    let out = response.headers_mut();
    for (name, value) in HEADERS {
        out.entry(name).or_insert(HeaderValue::from_static(value));
    }
    response
}

/// API responses are never cached.
pub(crate) async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    response
}

/// Refuses state-changing requests without the CSRF header or from another origin.
pub(crate) async fn csrf(request: Request, next: Next) -> Result<Response, ApiError> {
    // CONNECT opens WebSockets over HTTP/2 (RFC 8441), like the GET upgrade over HTTP/1.1:
    // only the WS hub accepts it, and it checks `Origin` itself.
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::CONNECT
    ) {
        return Ok(next.run(request).await);
    }
    let headers = request.headers();
    if headers.get(CSRF_HEADER).is_none_or(|value| value != "1") {
        return Err(ApiError::Forbidden(format!(
            "State-changing requests need the header {CSRF_HEADER}: 1"
        )));
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site == "cross-site")
    {
        return Err(ApiError::Forbidden(
            "Cross-site requests are refused".to_owned(),
        ));
    }
    check_origin(headers, request.extensions().get::<Client>())?;
    Ok(next.run(request).await)
}

/// Refuses a request whose `Origin`, when it sends one, is not ours.
pub(crate) fn check_origin(headers: &HeaderMap, client: Option<&Client>) -> Result<(), ApiError> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let ours = client.and_then(Client::origin);
    if ours
        .as_deref()
        .is_none_or(|ours| origin.as_bytes() != ours.as_bytes())
    {
        return Err(ApiError::Forbidden(format!(
            "Requests from {} are refused",
            String::from_utf8_lossy(origin.as_bytes())
        )));
    }
    Ok(())
}

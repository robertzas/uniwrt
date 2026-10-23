//! Security headers on every response (features/security.md): a strict CSP, no framing, no MIME
//! sniffing, no referrers and same-origin isolation. A handler may set its own value first.

use axum::{
    http::{HeaderName, HeaderValue, header},
    response::Response,
};

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

pub(crate) async fn headers(mut response: Response) -> Response {
    let out = response.headers_mut();
    for (name, value) in HEADERS {
        out.entry(name).or_insert(HeaderValue::from_static(value));
    }
    response
}

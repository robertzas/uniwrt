//! `GET /api/v1/tls` (what the HTTPS certificate covers, and the CA's fingerprint to compare on
//! a device) and `GET /api/v1/tls/ca.crt` (the local CA, for devices to trust). Both are public:
//! a client needs them before it can log in without a certificate warning.

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse as _, Response},
};

use crate::tls::Tls;

pub(crate) async fn info(State(tls): State<Tls>) -> Response {
    (
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(tls.info()),
    )
        .into_response()
}

/// PEM, which every platform imports; the `.crt` name and this type make phones offer to install it.
pub(crate) async fn ca(State(tls): State<Tls>) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/x-x509-ca-cert"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"uniwrt-ca.crt\""),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        tls.ca_pem(),
    )
        .into_response()
}

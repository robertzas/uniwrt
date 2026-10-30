//! `GET /api/v1/tls` (what the HTTPS certificate covers, and the CA's fingerprint to compare on
//! a device) and `GET /api/v1/tls/ca.crt` (the local CA, for devices to trust). Both are public:
//! a client needs them before it can log in without a certificate warning. The server's TLS
//! module provides them ([`Certificate`]).

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse as _, Response},
};
use serde::Serialize;
use ts_rs::TS;

use crate::Api;

/// The certificate the HTTPS listener serves, for clients to download and check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TlsInfo {
    /// The local CA's common name, as trust stores list it.
    pub ca_name: String,
    /// SHA-256 of the CA certificate, colon-separated uppercase hex, to compare on a device.
    pub ca_sha256: String,
    /// When the CA expires, RFC 3339 UTC.
    pub ca_not_after: String,
    /// The DNS names and IP addresses the certificate covers.
    pub names: Vec<String>,
    /// When the certificate expires (RFC 3339 UTC); it is renewed 30 days before.
    pub not_after: String,
}

/// The HTTPS certificate as it is now.
pub trait Certificate: Send + Sync + 'static {
    fn info(&self) -> TlsInfo;
    /// The CA certificate, PEM.
    fn ca_pem(&self) -> String;
}

pub(crate) async fn info(State(api): State<Api>) -> Json<TlsInfo> {
    Json(api.certificate().info())
}

/// PEM, which every platform imports; the `.crt` name and this type make phones offer to install it.
pub(crate) async fn ca(State(api): State<Api>) -> Response {
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
        ],
        api.certificate().ca_pem(),
    )
        .into_response()
}

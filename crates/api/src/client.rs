//! [`Client`]: who sent a request, as the server determined it. The listener the request came
//! through sets it (the server crate): over HTTPS from the connection, through a trusted reverse
//! proxy from its `X-Forwarded-*` headers. Rate limits, the audit log and the CSRF checks rely
//! on it.

use std::net::IpAddr;

use axum::{extract::FromRequestParts, http::request::Parts};

use crate::ApiError;

/// The client behind a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub ip: IpAddr,
    /// Whether the browser reached us over HTTPS (directly or through the proxy).
    pub secure: bool,
    /// The host (and port) the browser addressed, for checking `Origin`.
    pub host: Option<String>,
}

impl Client {
    /// The origin the browser's own pages have: `https://<host>`.
    pub(crate) fn origin(&self) -> Option<String> {
        let scheme = if self.secure { "https" } else { "http" };
        self.host.as_ref().map(|host| format!("{scheme}://{host}"))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Client {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(parts.extensions.get::<Self>().cloned().ok_or_else(|| {
            ApiError::Internal("the listener did not identify the client".to_owned())
        }))
    }
}

//! HTTP(S) POSTs for ntfy and webhooks: hyper over rustls (ring) with Mozilla's roots, HTTP/1.1,
//! a timeout on the whole exchange, redirects not followed, and a bounded read of the answer.

use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    Request, Uri,
    header::{CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::ClientConfig;

use crate::{SendError, render::HttpRequest};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How much of a failed answer is kept for the error.
const MAX_ERROR_BODY: usize = 4096;
const SHOWN_ERROR_BODY: usize = 200;

pub(crate) struct Http {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    user_agent: HeaderValue,
    timeout: Duration,
}

impl Http {
    pub(crate) fn new(tls: ClientConfig, user_agent: &str, timeout: Duration) -> Self {
        let mut connector = HttpConnector::new();
        connector.enforce_http(false);
        connector.set_connect_timeout(Some(CONNECT_TIMEOUT));
        let https = HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .wrap_connector(connector);
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(1)
            .build(https);
        Self {
            client,
            user_agent: HeaderValue::from_str(user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static("uniwrt")),
            timeout,
        }
    }

    pub(crate) async fn post(&self, request: &HttpRequest) -> Result<(), SendError> {
        let uri: Uri = request
            .url
            .parse()
            .map_err(|_| SendError::Invalid("the URL is not valid".to_owned()))?;
        let mut builder = Request::post(uri)
            .header(CONTENT_TYPE, "application/json")
            .header(USER_AGENT, self.user_agent.clone());
        for (name, value) in &request.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| SendError::Invalid(format!("{name} is not a header name")))?;
            let mut value = HeaderValue::from_str(value)
                .map_err(|_| SendError::Invalid(format!("the value of {name} is not valid")))?;
            value.set_sensitive(true);
            builder = builder.header(name, value);
        }
        let request = builder
            .body(Full::new(Bytes::from(request.body.clone())))
            .map_err(|err| SendError::Invalid(err.to_string()))?;
        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|err| SendError::Connect(chain(&err)))?;
            let status = response.status();
            if status.is_success() {
                return Ok(());
            }
            let body = Limited::new(response.into_body(), MAX_ERROR_BODY)
                .collect()
                .await
                .map(http_body_util::Collected::to_bytes)
                .unwrap_or_default();
            let body = String::from_utf8_lossy(&body);
            let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
            Err(SendError::Status {
                status: status.as_u16(),
                body: body.chars().take(SHOWN_ERROR_BODY).collect(),
            })
        };
        tokio::time::timeout(self.timeout, exchange)
            .await
            .map_err(|_| SendError::Timeout(self.timeout.as_secs()))?
    }
}

/// An error and its causes: hyper's own say little (`client error (Connect)`).
pub(crate) fn chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

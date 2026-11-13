//! HTTP(S) GETs and JSON POSTs to ASU and OpenWrt's download server: hyper over rustls (ring)
//! with Mozilla's roots, HTTP/1.1, a timeout on the whole exchange, redirects not followed,
//! and a bounded read of the answer.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    Request, Uri,
    header::{ACCEPT, CONTENT_TYPE, HeaderValue, USER_AGENT},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{ClientConfig, RootCertStore};

use crate::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The whole exchange, the answer read included (a release's catalog is about 400 KB).
const TIMEOUT: Duration = Duration::from_secs(60);

/// An answer: its status and its body.
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: Bytes,
}

pub(crate) struct Http {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    user_agent: HeaderValue,
}

impl Http {
    pub(crate) fn new(user_agent: &str) -> Result<Self, Error> {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|err| Error::Invalid(format!("TLS: {err}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
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
            .pool_max_idle_per_host(2)
            .build(https);
        Ok(Self {
            client,
            user_agent: HeaderValue::from_str(user_agent)
                .unwrap_or_else(|_| HeaderValue::from_static("uniwrt")),
        })
    }

    pub(crate) async fn get(&self, url: &str, max: usize) -> Result<Answer, Error> {
        self.exchange(url, None, max).await
    }

    pub(crate) async fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
        max: usize,
    ) -> Result<Answer, Error> {
        self.exchange(url, Some(body), max).await
    }

    async fn exchange(
        &self,
        url: &str,
        body: Option<Vec<u8>>,
        max: usize,
    ) -> Result<Answer, Error> {
        let uri: Uri = url
            .parse()
            .map_err(|_| Error::Invalid(format!("{url} is not a URL")))?;
        let builder = Request::builder()
            .uri(uri)
            .header(USER_AGENT, self.user_agent.clone())
            .header(ACCEPT, "application/json");
        let request = match body {
            Some(body) => builder
                .method("POST")
                .header(CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(body))),
            None => builder.method("GET").body(Full::new(Bytes::new())),
        }
        .map_err(|err| Error::Invalid(err.to_string()))?;
        let unreachable = |why: String| Error::Unreachable {
            url: url.to_owned(),
            why,
        };
        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|err| unreachable(chain(&err)))?;
            let status = response.status().as_u16();
            let body = Limited::new(response.into_body(), max)
                .collect()
                .await
                .map_err(|err| unreachable(err.to_string()))?
                .to_bytes();
            Ok(Answer { status, body })
        };
        tokio::time::timeout(TIMEOUT, exchange)
            .await
            .map_err(|_| unreachable(format!("no answer within {} s", TIMEOUT.as_secs())))?
    }
}

/// An error and its causes: hyper's own say little (`client error (Connect)`).
fn chain(err: &(dyn std::error::Error + 'static)) -> String {
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

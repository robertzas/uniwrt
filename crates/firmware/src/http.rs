//! HTTP(S) GETs and JSON POSTs to ASU, OpenWrt's download server and GitHub's API: hyper over rustls (ring)
//! with Mozilla's roots, HTTP/1.1, a timeout on the whole exchange, redirects not followed,
//! and a bounded read of the answer; downloads ([`Http::download`]) follow redirects and stream
//! to a file while hashed.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    HeaderMap, Request, Uri,
    header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue, LOCATION, USER_AGENT},
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
/// A whole download: the controller's tarball is about 15 MB.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(15);
const MAX_REDIRECTS: usize = 5;

/// An answer: its status, its headers and its body.
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) headers: HeaderMap,
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
        self.exchange(url, None, &[], max).await
    }

    /// A GET with more headers (GitHub's version, a token).
    pub(crate) async fn get_with(
        &self,
        url: &str,
        headers: &[(HeaderName, HeaderValue)],
        max: usize,
    ) -> Result<Answer, Error> {
        self.exchange(url, None, headers, max).await
    }

    pub(crate) async fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
        max: usize,
    ) -> Result<Answer, Error> {
        self.exchange(url, Some(body), &[], max).await
    }

    async fn exchange(
        &self,
        url: &str,
        body: Option<Vec<u8>>,
        headers: &[(HeaderName, HeaderValue)],
        max: usize,
    ) -> Result<Answer, Error> {
        let uri: Uri = url
            .parse()
            .map_err(|_| Error::Invalid(format!("{url} is not a URL")))?;
        let mut builder = Request::builder()
            .uri(uri)
            .header(USER_AGENT, self.user_agent.clone())
            .header(ACCEPT, "application/json");
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
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
            let headers = response.headers().clone();
            let body = Limited::new(response.into_body(), max)
                .collect()
                .await
                .map_err(|err| unreachable(err.to_string()))?
                .to_bytes();
            Ok(Answer {
                status,
                headers,
                body,
            })
        };
        tokio::time::timeout(TIMEOUT, exchange)
            .await
            .map_err(|_| unreachable(format!("no answer within {} s", TIMEOUT.as_secs())))?
    }
}

/// A file downloaded: its SHA-256 and size.
pub(crate) struct Downloaded {
    pub(crate) sha256: [u8; 32],
    pub(crate) size: u64,
}

impl Http {
    /// Downloads `url` to `to` (created or truncated), following up to five redirects to HTTPS
    /// (or plain HTTP from a plain HTTP URL), at most `max` bytes, within [`DOWNLOAD_TIMEOUT`].
    pub(crate) async fn download(
        &self,
        url: &str,
        max: u64,
        to: &std::path::Path,
    ) -> Result<Downloaded, Error> {
        tokio::time::timeout(DOWNLOAD_TIMEOUT, self.download_within(url, max, to))
            .await
            .map_err(|_| Error::Unreachable {
                url: url.to_owned(),
                why: format!(
                    "not downloaded within {} min",
                    DOWNLOAD_TIMEOUT.as_secs() / 60
                ),
            })?
    }

    async fn download_within(
        &self,
        url: &str,
        max: u64,
        to: &std::path::Path,
    ) -> Result<Downloaded, Error> {
        use tokio::io::AsyncWriteExt as _;

        let unreachable = |at: &str, why: String| Error::Unreachable {
            url: at.to_owned(),
            why,
        };
        let plain = url.starts_with("http://");
        let mut at = url.to_owned();
        let response = 'redirects: {
            for _ in 0..=MAX_REDIRECTS {
                let uri: Uri = at
                    .parse()
                    .map_err(|_| Error::Invalid(format!("{at} is not a URL")))?;
                let request = Request::builder()
                    .uri(uri)
                    .header(USER_AGENT, self.user_agent.clone())
                    .header(ACCEPT, "application/octet-stream")
                    .body(Full::new(Bytes::new()))
                    .map_err(|err| Error::Invalid(err.to_string()))?;
                let response = self
                    .client
                    .request(request)
                    .await
                    .map_err(|err| unreachable(&at, chain(&err)))?;
                if !response.status().is_redirection() {
                    break 'redirects response;
                }
                let next = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .filter(|next| {
                        next.starts_with("https://") || (plain && next.starts_with("http://"))
                    })
                    .ok_or_else(|| {
                        Error::Invalid(format!("{at} redirects nowhere uniwrt follows"))
                    })?;
                at = next.to_owned();
            }
            return Err(Error::Invalid(format!("{url} redirects too often")));
        };
        let status = response.status().as_u16();
        if status != 200 {
            return Err(Error::Status { url: at, status });
        }
        let mut file = tokio::fs::File::create(to)
            .await
            .map_err(|err| Error::Storage(format!("{}: {err}", to.display())))?;
        let mut body = response.into_body();
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut size = 0_u64;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|err| unreachable(&at, chain(&err)))?;
            let Ok(data) = frame.into_data() else {
                continue;
            };
            size += data.len() as u64;
            if size > max {
                return Err(Error::Invalid(format!(
                    "{url} is larger than {} MiB",
                    max >> 20
                )));
            }
            hash.update(&data);
            file.write_all(&data)
                .await
                .map_err(|err| Error::Storage(format!("{}: {err}", to.display())))?;
        }
        file.sync_all()
            .await
            .map_err(|err| Error::Storage(format!("{}: {err}", to.display())))?;
        let mut sha256 = [0; 32];
        sha256.copy_from_slice(hash.finish().as_ref());
        Ok(Downloaded { sha256, size })
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

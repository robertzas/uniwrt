//! Where lists come from: [`Source`], over HTTP(S) in production ([`HttpSource`]: hyper over
//! rustls (ring) with Mozilla's roots, HTTP/1.1, conditional GETs with the copy's `ETag` and
//! `Last-Modified`, up to five redirects to HTTPS, at most [`MAX_LIST`] bytes, a timeout on the
//! whole exchange).

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Empty, Limited};
use hyper::{
    Request, StatusCode, Uri,
    header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, USER_AGENT},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use rustls::{ClientConfig, RootCertStore};

use crate::Error;

/// The largest list taken (OISD Big's dnsmasq file is about 8 MB).
pub const MAX_LIST: usize = 32 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(90);
const MAX_REDIRECTS: usize = 5;

/// What the copy kept says about itself, for a conditional GET.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// What a source answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The copy kept is current.
    NotModified,
    /// A new copy, and what identifies it.
    Body { body: Bytes, validators: Validators },
}

/// A boxed future, so that [`Source`] works behind `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where lists come from.
pub trait Source: Send + Sync + 'static {
    /// Gets `url`, unless the copy that `validators` describe is current.
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
    ) -> BoxFuture<'a, Result<Fetched, Error>>;
}

/// Lists over HTTP(S).
pub struct HttpSource {
    client: Client<HttpsConnector<HttpConnector>, Empty<Bytes>>,
    user_agent: String,
}

impl HttpSource {
    pub fn new(user_agent: &str) -> Result<Arc<Self>, Error> {
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
        Ok(Arc::new(Self {
            client,
            user_agent: user_agent.to_owned(),
        }))
    }

    async fn get(&self, url: &str, validators: &Validators) -> Result<Fetched, Error> {
        let unreachable = |at: &str, why: String| Error::Unreachable {
            url: at.to_owned(),
            why,
        };
        let plain = url.starts_with("http://");
        let mut at = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let uri: Uri = at
                .parse()
                .map_err(|_| Error::Invalid(format!("{at} is not a URL")))?;
            let mut request = Request::get(uri).header(USER_AGENT, self.user_agent.as_str());
            if let Some(etag) = &validators.etag {
                request = request.header(IF_NONE_MATCH, etag.as_str());
            }
            if let Some(since) = &validators.last_modified {
                request = request.header(IF_MODIFIED_SINCE, since.as_str());
            }
            let request = request
                .body(Empty::new())
                .map_err(|err| Error::Invalid(err.to_string()))?;
            let response = self
                .client
                .request(request)
                .await
                .map_err(|err| unreachable(&at, chain(&err)))?;
            let status = response.status();
            if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
                at = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .filter(|next| {
                        next.starts_with("https://") || (plain && next.starts_with("http://"))
                    })
                    .ok_or_else(|| {
                        Error::Invalid(format!("{at} redirects nowhere uniwrt follows"))
                    })?
                    .to_owned();
                continue;
            }
            if status == StatusCode::NOT_MODIFIED {
                return Ok(Fetched::NotModified);
            }
            if status != StatusCode::OK {
                return Err(Error::Status {
                    url: at,
                    status: status.as_u16(),
                });
            }
            let header = |name| {
                response
                    .headers()
                    .get(name)
                    .and_then(|value: &hyper::header::HeaderValue| value.to_str().ok())
                    .map(str::to_owned)
            };
            let validators = Validators {
                etag: header(ETAG),
                last_modified: header(LAST_MODIFIED),
            };
            let body = Limited::new(response.into_body(), MAX_LIST)
                .collect()
                .await
                .map_err(|err| {
                    if err.is::<http_body_util::LengthLimitError>() {
                        Error::TooLarge {
                            url: at.clone(),
                            max_mib: MAX_LIST >> 20,
                        }
                    } else {
                        unreachable(&at, err.to_string())
                    }
                })?
                .to_bytes();
            return Ok(Fetched::Body { body, validators });
        }
        Err(Error::Invalid(format!("{url} redirects too often")))
    }
}

impl Source for HttpSource {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        Box::pin(async move {
            tokio::time::timeout(TIMEOUT, self.get(url, validators))
                .await
                .map_err(|_| Error::Unreachable {
                    url: url.to_owned(),
                    why: format!("not downloaded within {} s", TIMEOUT.as_secs()),
                })?
        })
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

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;

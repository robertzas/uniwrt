//! Where feeds come from ([`Source`]): the upstream servers over HTTP(S) ([`HttpSource`]:
//! hyper with rustls and Mozilla's roots, conditional GETs, a file streamed to disk as it
//! arrives and stopped past its size limit), or a fake in tests and demo mode.

use std::{future::Future, path::Path, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Empty};
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
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;

use crate::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a download may go without a byte.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// How long one download may take: a large package over a slow line.
const TIMEOUT: Duration = Duration::from_mins(30);
const MAX_REDIRECTS: usize = 5;

/// What the copy kept says about itself, for a conditional GET.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Validators {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

/// What a source answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The copy kept is current; nothing was written.
    NotModified,
    /// The file was written whole, `bytes` long.
    Written { bytes: u64, validators: Validators },
}

/// A boxed future, so that [`Source`] works behind `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where feeds come from.
pub trait Source: Send + Sync + 'static {
    /// Writes `url` to `dest` (made or replaced), at most `max` bytes, unless the copy that
    /// `validators` describe is current. A failure may leave `dest` partly written.
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
        dest: &'a Path,
        max: u64,
    ) -> BoxFuture<'a, Result<Fetched, Error>>;
}

/// Feeds over HTTP(S).
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
            .pool_max_idle_per_host(4)
            .build(https);
        Ok(Arc::new(Self {
            client,
            user_agent: user_agent.to_owned(),
        }))
    }

    async fn get(
        &self,
        url: &str,
        validators: &Validators,
        dest: &Path,
        max: u64,
    ) -> Result<Fetched, Error> {
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
            let storage =
                |err: std::io::Error| Error::Storage(format!("{}: {err}", dest.display()));
            let mut file = tokio::fs::File::create(dest).await.map_err(storage)?;
            let mut body = response.into_body();
            let mut bytes = 0u64;
            loop {
                let frame = tokio::time::timeout(IDLE_TIMEOUT, body.frame())
                    .await
                    .map_err(|_| {
                        unreachable(
                            &at,
                            format!("nothing arrived for {} s", IDLE_TIMEOUT.as_secs()),
                        )
                    })?;
                let Some(frame) = frame else { break };
                let frame = frame.map_err(|err| unreachable(&at, chain(&err)))?;
                let Ok(data) = frame.into_data() else {
                    continue;
                };
                bytes += data.len() as u64;
                if bytes > max {
                    return Err(Error::TooLarge {
                        url: at,
                        max_mib: max >> 20,
                    });
                }
                file.write_all(&data).await.map_err(storage)?;
            }
            file.flush().await.map_err(storage)?;
            file.sync_all().await.map_err(storage)?;
            return Ok(Fetched::Written { bytes, validators });
        }
        Err(Error::Invalid(format!("{url} redirects too often")))
    }
}

impl Source for HttpSource {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        validators: &'a Validators,
        dest: &'a Path,
        max: u64,
    ) -> BoxFuture<'a, Result<Fetched, Error>> {
        Box::pin(async move {
            tokio::time::timeout(TIMEOUT, self.get(url, validators, dest, max))
                .await
                .map_err(|_| Error::Unreachable {
                    url: url.to_owned(),
                    why: format!("not downloaded within {} min", TIMEOUT.as_secs() / 60),
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

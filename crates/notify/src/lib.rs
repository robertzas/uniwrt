//! Notifiers (features/observability.md, Notifications; D-061, D-063): alerts in words, rendered
//! for ntfy, webhooks and email, sent over HTTP(S) and SMTP, and the policy that decides what to
//! send and when.
//!
//! - **API:** [`Policy`] (pure: [`Policy::alerts`] as the alert engine stores them,
//!   [`Policy::tick`] for reminders, [`Policy::set_notifiers`], [`Policy::resume`] after a
//!   restart) yields [`Send`]s; [`Item`] tells of one alert ([`Kind`]: opened, reopened,
//!   reminder, resolved) in words ([`Names`]), and [`Message`] is one item, a digest of several,
//!   or a test; [`Sender::send`] renders a message for a notifier's [`NotifierSpec`] with its
//!   [`Secrets`] (the vault's `notifier/<id>`) and delivers it, failing with a [`SendError`]
//!   that says whether trying again may help; [`validate`] checks a notifier before it is
//!   stored, [`check_url`] an address.
//! - **Formats:** ntfy's JSON publishing (priority and tag by level, the alert as the click
//!   link); webhooks as uniwrt's JSON (also for Home Assistant), a Discord embed (no mentions),
//!   Slack blocks, a Teams Workflows Adaptive Card, or the user's JSON template with
//!   `{{placeholders}}` ([`PLACEHOLDERS`]); email as plain text and HTML.
//! - **Transport:** hyper over rustls with the ring provider and Mozilla's roots
//!   (webpki-roots; nothing read from the system, so it works in a `FROM scratch` image),
//!   HTTP/1.1, 15 s for a whole exchange, redirects not followed; lettre for SMTP with STARTTLS
//!   required or TLS from the start, never in the clear.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, hyper, hyper-util, hyper-rustls,
//!   http-body-util, rustls, webpki-roots, lettre, tokio (timeouts), `serde_json`, zeroize.
//! - **Tests:** the policy over hand-made alerts; each format's JSON; templates; an HTTP and an
//!   HTTPS server and a STARTTLS SMTP server on loopback, trusted through
//!   [`Sender::with_roots`] (`cargo nextest run -p uniwrt-notify`).

mod http;
mod message;
mod policy;
mod render;
mod secrets;
mod smtp;
pub mod text;
mod validate;

use std::{sync::Arc, time::Duration};

use rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer};
use uniwrt_model::notify::NotifierSpec;

pub use self::{
    message::{Item, Kind, Message},
    policy::{CARRY_OVER, Policy, Send},
    render::{
        Email, HttpRequest, MAX_TEMPLATE, PLACEHOLDERS, RenderError, Rendered, generic, render,
    },
    secrets::{Header, Secrets, SecretsError},
    text::Names,
    validate::{MAX_HEADERS, MAX_NAME, MAX_NODES, MAX_RECIPIENTS, check_url, validate},
};

/// How long one delivery may take.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// Why a delivery failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// The notifier cannot work as it is (no retry helps).
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error("no answer within {0} s")]
    Timeout(u64),
    #[error("cannot reach the server: {0}")]
    Connect(String),
    #[error("the server answered HTTP {status}{}", body_suffix(.body))]
    Status { status: u16, body: String },
    #[error("SMTP: {message}")]
    Smtp { message: String, transient: bool },
}

fn body_suffix(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        format!(": {body}")
    }
}

impl SendError {
    /// Whether trying again later may succeed.
    pub const fn retryable(&self) -> bool {
        match self {
            Self::Timeout(_) | Self::Connect(_) => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            Self::Smtp { transient, .. } => *transient,
            Self::Invalid(_) | Self::Render(_) => false,
        }
    }
}

/// Delivers messages. Clones share its HTTP connections.
#[derive(Clone)]
pub struct Sender {
    inner: Arc<Inner>,
}

struct Inner {
    http: http::Http,
    extra_roots: Vec<CertificateDer<'static>>,
    timeout: Duration,
}

impl Sender {
    /// A sender that trusts Mozilla's roots, introducing itself as `user_agent`.
    pub fn new(user_agent: &str) -> Result<Self, SendError> {
        Self::with_roots(user_agent, Vec::new())
    }

    /// One that also trusts `extra_roots` (tests' own CAs).
    pub fn with_roots(
        user_agent: &str,
        extra_roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, SendError> {
        let mut roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        for cert in &extra_roots {
            roots
                .add(cert.clone())
                .map_err(|err| SendError::Invalid(format!("TLS: {err}")))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|err| SendError::Invalid(format!("TLS: {err}")))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            inner: Arc::new(Inner {
                http: http::Http::new(tls, user_agent, TIMEOUT),
                extra_roots,
                timeout: TIMEOUT,
            }),
        })
    }

    /// Renders `message` for the notifier and delivers it.
    pub async fn send(
        &self,
        spec: &NotifierSpec,
        secrets: &Secrets,
        message: &Message,
    ) -> Result<(), SendError> {
        match (spec, render(spec, secrets, message)?) {
            (_, Rendered::Http(request)) => self.inner.http.post(&request).await,
            (NotifierSpec::Email(email_spec), Rendered::Email(email)) => {
                let inner = &self.inner;
                smtp::send(
                    email_spec,
                    secrets,
                    &email,
                    &inner.extra_roots,
                    inner.timeout,
                )
                .await
            }
            (_, Rendered::Email(_)) => Err(SendError::Invalid("not an email notifier".to_owned())),
        }
    }
}

#[cfg(test)]
mod tests;

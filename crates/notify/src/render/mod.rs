//! A [`Message`] rendered for a notifier: an HTTP request for ntfy and webhooks (each service's
//! own format, uniwrt's JSON or the user's template), or an email's subject and bodies.

mod email;
mod ntfy;
mod template;
mod webhook;

use uniwrt_model::{
    events::EventLevel,
    notify::{NotifierSpec, WebhookFormat},
};
use zeroize::Zeroizing;

pub(crate) use self::template::check as check_template;
pub use self::{
    email::Email,
    template::{MAX_TEMPLATE, PLACEHOLDERS},
    webhook::generic,
};
use crate::{Message, Secrets, message::Kind};

/// An HTTP POST to make.
#[derive(Debug)]
pub struct HttpRequest {
    pub url: Zeroizing<String>,
    /// Besides `Content-Type: application/json`; values may be secrets.
    pub headers: Vec<(String, Zeroizing<String>)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum Rendered {
    Http(HttpRequest),
    Email(Email),
}

/// What cannot be rendered: a secret missing, or a template that does not make JSON.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("the webhook has no URL")]
    NoUrl,
    #[error("the template {0}")]
    Template(String),
}

pub fn render(
    spec: &NotifierSpec,
    secrets: &Secrets,
    message: &Message,
) -> Result<Rendered, RenderError> {
    match spec {
        NotifierSpec::Ntfy(spec) => Ok(Rendered::Http(ntfy::request(spec, secrets, message))),
        NotifierSpec::Email(_) => Ok(Rendered::Email(email::render(message))),
        NotifierSpec::Webhook(spec) => {
            let url = secrets.url.clone().ok_or(RenderError::NoUrl)?;
            let body = match spec.format {
                WebhookFormat::Generic | WebhookFormat::HomeAssistant => {
                    webhook::generic(message).to_string().into_bytes()
                }
                WebhookFormat::Discord => webhook::discord(message).to_string().into_bytes(),
                WebhookFormat::Slack => webhook::slack(message).to_string().into_bytes(),
                WebhookFormat::Teams => webhook::teams(message).to_string().into_bytes(),
                WebhookFormat::Custom => {
                    template::render(spec.template.as_deref().unwrap_or_default(), message)
                        .map_err(RenderError::Template)?
                        .into_bytes()
                }
            };
            let headers = secrets
                .headers
                .iter()
                .map(|h| (h.name.clone(), Zeroizing::new(h.value.clone())))
                .collect();
            Ok(Rendered::Http(HttpRequest {
                url: Zeroizing::new(url),
                headers,
                body,
            }))
        }
    }
}

/// A message's colour as RGB: green when it only tells of resolutions, else its level's.
fn color(message: &Message) -> u32 {
    if message.resolved() {
        return 0x18_80_38;
    }
    if matches!(message.kind, Kind::Test) {
        return 0x1A_73_E8;
    }
    match message.level {
        EventLevel::Critical => 0xD9_30_25,
        EventLevel::Warning => 0xE3_74_00,
        EventLevel::Notice => 0x1A_73_E8,
        EventLevel::Info => 0x5F_63_68,
    }
}

/// What a message says of itself in a footer or a badge.
fn label(message: &Message) -> &'static str {
    if message.resolved() {
        return "Resolved";
    }
    match message.kind {
        Kind::Test => "Test",
        _ => crate::text::level(message.level),
    }
}

/// At most `max` characters, an ellipsis ending what was cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests;

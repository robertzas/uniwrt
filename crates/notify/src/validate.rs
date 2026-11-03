//! What a notifier must be to be stored: checked as the API takes it, so that sending fails only
//! for reasons outside uniwrt (a server down, a password changed).

use hyper::{Uri, header::HeaderName};
use lettre::message::Mailbox;
use uniwrt_core::{AlertId, NodeId, RuleId, Timestamp};
use uniwrt_model::{
    alerts::{AlertCause, AlertRecord, AlertState, Comparison},
    events::EventLevel,
    metric::Metric,
    notify::{NotifierConfig, NotifierFilter, NotifierSpec, WebhookFormat},
};

use crate::{
    Message, Names, Secrets,
    message::{Item, Kind},
    render::check_template,
};

pub const MAX_NAME: usize = 60;
pub const MAX_NODES: usize = 32;
pub const MAX_RECIPIENTS: usize = 10;
pub const MAX_HEADERS: usize = 10;
const MAX_URL: usize = 2048;
const MAX_FIELD: usize = 256;
/// Headers the request sets itself.
const RESERVED: [&str; 6] = [
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
    "user-agent",
];

/// Why `config` with `secrets` cannot be stored, in words for the user.
pub fn validate(config: &NotifierConfig, secrets: &Secrets) -> Result<(), String> {
    let name = config.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.contains(char::is_control) {
        return Err(format!(
            "A name has 1 to {MAX_NAME} characters, on one line"
        ));
    }
    filter(&config.filter)?;
    match &config.spec {
        NotifierSpec::Ntfy(ntfy) => {
            check_url(&ntfy.server).map_err(|why| format!("The ntfy server {why}"))?;
            let topic_ok = !ntfy.topic.is_empty()
                && ntfy.topic.len() <= 64
                && ntfy
                    .topic
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !topic_ok {
                return Err("A topic has 1 to 64 letters, digits, - or _".to_owned());
            }
            field("The username", ntfy.username.as_deref())?;
            if ntfy.username.is_some() && secrets.password.is_none() {
                return Err("A username needs its password".to_owned());
            }
        }
        NotifierSpec::Webhook(webhook) => {
            let url = secrets.url.as_deref().ok_or("A webhook needs its URL")?;
            check_url(url).map_err(|why| format!("The webhook's URL {why}"))?;
            if secrets.headers.len() > MAX_HEADERS {
                return Err(format!("A webhook has at most {MAX_HEADERS} headers"));
            }
            for header in &secrets.headers {
                let name = HeaderName::from_bytes(header.name.as_bytes())
                    .map_err(|_| format!("{:?} is not a header name", header.name))?;
                if RESERVED.contains(&name.as_str()) {
                    return Err(format!("uniwrt sets {} itself", header.name));
                }
                if hyper::header::HeaderValue::from_str(&header.value).is_err()
                    || header.value.len() > MAX_URL
                {
                    return Err(format!("The value of {} is not valid", header.name));
                }
            }
            if webhook.format == WebhookFormat::Custom {
                let template = webhook.template.as_deref().unwrap_or_default();
                check_template(template, &sample()).map_err(|why| format!("The template {why}"))?;
            }
        }
        NotifierSpec::Email(email) => {
            let host_ok = !email.host.is_empty()
                && email.host.len() <= 253
                && !email.host.contains(|c: char| c.is_whitespace() || c == '/');
            if !host_ok {
                return Err("The SMTP server is not a host name or address".to_owned());
            }
            if email.port == 0 {
                return Err("The SMTP port is 1 to 65535".to_owned());
            }
            field("The username", email.username.as_deref())?;
            if email.username.is_some() && secrets.password.is_none() {
                return Err("A username needs its password".to_owned());
            }
            mailbox(&email.from)?;
            if email.to.is_empty() || email.to.len() > MAX_RECIPIENTS {
                return Err(format!("An email goes to 1 to {MAX_RECIPIENTS} addresses"));
            }
            for to in &email.to {
                mailbox(to)?;
            }
        }
    }
    Ok(())
}

fn filter(filter: &NotifierFilter) -> Result<(), String> {
    if filter.nodes.len() > MAX_NODES {
        return Err(format!("A notifier names at most {MAX_NODES} devices"));
    }
    if let Some(s) = filter.remind_s
        && !(NotifierFilter::MIN_REMIND_S..=NotifierFilter::MAX_REMIND_S).contains(&s)
    {
        return Err("Reminders come every 15 minutes to 7 days".to_owned());
    }
    Ok(())
}

fn field(what: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value)
            if value.is_empty() || value.len() > MAX_FIELD || value.contains(char::is_control) =>
        {
            Err(format!("{what} has 1 to {MAX_FIELD} characters"))
        }
        _ => Ok(()),
    }
}

fn mailbox(text: &str) -> Result<(), String> {
    text.parse::<Mailbox>()
        .map(|_| ())
        .map_err(|_| format!("{text:?} is not an email address"))
}

/// Why `text` is not an `http://` or `https://` URL with a host (`"is not …"`).
pub fn check_url(text: &str) -> Result<(), String> {
    let bad = || Err("is not an http:// or https:// URL".to_owned());
    if text.len() > MAX_URL || text.contains(char::is_whitespace) {
        return bad();
    }
    let Ok(uri) = text.parse::<Uri>() else {
        return bad();
    };
    match (uri.scheme_str(), uri.host()) {
        (Some("http" | "https"), Some(host)) if !host.is_empty() => Ok(()),
        _ => bad(),
    }
}

/// An alert to try templates on.
pub(crate) fn sample() -> Message {
    let at = Timestamp::from_unix_ms(1_793_628_120_000);
    let alert = AlertRecord {
        id: AlertId::new(1),
        rule: RuleId::new(1),
        state: AlertState {
            name: "CPU high".to_owned(),
            level: EventLevel::Warning,
            subject: "node:1".to_owned(),
            node: Some(NodeId::new(1)),
            client: None,
            started_at: at,
            last_at: at,
            resolved_at: None,
            resolution: None,
            count: 1,
            silenced: false,
            cause: AlertCause::Metric {
                metric: Metric::Cpu,
                op: Comparison::Above,
                threshold: 90.0,
                value: 93.0,
                worst: 97.0,
            },
        },
        acknowledged: None,
    };
    let names = Names::default();
    let item = Item::new(Kind::Opened, alert, &names, Some("https://uniwrt.example"));
    Message::new(vec![item], Some("https://uniwrt.example"), at)
}

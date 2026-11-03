//! ntfy's JSON publishing (`POST <server>` with the topic in the body), which carries any
//! title as UTF-8, unlike its headers.

use data_encoding::BASE64;
use serde_json::json;
use uniwrt_model::{events::EventLevel, notify::NtfySpec};
use zeroize::Zeroizing;

use super::{HttpRequest, truncate};
use crate::{Message, Secrets, message::Kind};

/// ntfy keeps messages to 4,096 bytes; characters are close enough.
const MAX_MESSAGE: usize = 4000;

pub(super) fn request(spec: &NtfySpec, secrets: &Secrets, message: &Message) -> HttpRequest {
    let (priority, tag) = if message.resolved() {
        (3, "white_check_mark")
    } else if message.kind == Kind::Test {
        (3, "bell")
    } else {
        match message.level {
            EventLevel::Critical => (5, "rotating_light"),
            EventLevel::Warning => (4, "warning"),
            EventLevel::Notice => (3, "information_source"),
            EventLevel::Info => (2, "speech_balloon"),
        }
    };
    let mut body = json!({
        "topic": spec.topic,
        "title": message.title,
        "message": truncate(&message.text, MAX_MESSAGE),
        "priority": priority,
        "tags": [tag],
    });
    if let Some(link) = &message.link {
        body["click"] = json!(link);
    }
    let authorization = match (&spec.username, &secrets.password, &secrets.token) {
        (Some(user), password, _) => {
            let pair = Zeroizing::new(format!("{user}:{}", password.as_deref().unwrap_or("")));
            Some(format!("Basic {}", BASE64.encode(pair.as_bytes())))
        }
        (None, _, Some(token)) => Some(format!("Bearer {token}")),
        (None, _, None) => None,
    };
    HttpRequest {
        url: Zeroizing::new(spec.server.trim_end_matches('/').to_owned()),
        headers: authorization
            .map(|value| ("Authorization".to_owned(), Zeroizing::new(value)))
            .into_iter()
            .collect(),
        body: body.to_string().into_bytes(),
    }
}

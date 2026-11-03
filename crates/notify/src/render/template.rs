//! A custom webhook's body: JSON with `{{placeholders}}`. Text placeholders are JSON-escaped
//! without quotes (write `"{{title}}"`); number placeholders are digits, or `null` when the
//! message has none (write `{{node}}`). A template must make JSON both for an alert and for a
//! test, which has none ([`check`]).

use std::fmt::Write as _;

use uniwrt_core::NodeId;

use crate::{Message, text};

/// The placeholders, with whether each is a number.
pub const PLACEHOLDERS: [(&str, bool); 13] = [
    ("title", false),
    ("message", false),
    ("level", false),
    ("event", false),
    ("url", false),
    ("at", true),
    ("at_iso", false),
    ("alerts", true),
    ("alert_id", true),
    ("name", false),
    ("subject", false),
    ("node", true),
    ("cause", false),
];

/// The longest template.
pub const MAX_TEMPLATE: usize = 8 * 1024;

/// Fills the template from `message`; why it cannot.
pub(super) fn render(template: &str, message: &Message) -> Result<String, String> {
    let mut out = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| "has a {{ without its }}".to_owned())?;
        let name = after[..end].trim();
        push(&mut out, name, message)?;
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    serde_json::from_str::<serde_json::Value>(&out)
        .map_err(|err| format!("does not make JSON: {err}"))?;
    Ok(out)
}

fn push(out: &mut String, name: &str, message: &Message) -> Result<(), String> {
    let item = message.items.first();
    let state = item.map(|item| &item.alert.state);
    let text = |value: &str| {
        let quoted = serde_json::Value::String(value.to_owned()).to_string();
        quoted[1..quoted.len() - 1].to_owned()
    };
    let number = |value: Option<i64>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let value = match name {
        "title" => text(&message.title),
        "message" => text(&message.text),
        "level" => text(text::level_name(message.level)),
        "event" => text(message.kind.name()),
        "url" => text(message.link.as_deref().unwrap_or_default()),
        "at" => number(Some(message.at.as_unix_ms())),
        "at_iso" => text(&text::rfc3339(message.at)),
        "alerts" => number(i64::try_from(message.items.len()).ok()),
        "alert_id" => number(item.map(|item| item.alert.id.get())),
        "name" => text(state.map_or("", |s| s.name.as_str())),
        "subject" => text(item.map_or("", |item| item.subject.as_str())),
        "node" => number(state.and_then(|s| s.node).map(NodeId::get)),
        "cause" => text(
            item.and_then(|item| item.cause.as_deref())
                .unwrap_or_default(),
        ),
        other => return Err(format!("has no placeholder {{{{{other}}}}}")),
    };
    let _ = write!(out, "{value}");
    Ok(())
}

/// Whether the template makes JSON for `sample` (an alert) and for a test; why not.
pub(crate) fn check(template: &str, sample: &Message) -> Result<(), String> {
    if template.trim().is_empty() {
        return Err("is empty".to_owned());
    }
    if template.len() > MAX_TEMPLATE {
        return Err(format!("is longer than {MAX_TEMPLATE} bytes"));
    }
    render(template, sample)?;
    render(template, &Message::test(None, sample.at))?;
    Ok(())
}

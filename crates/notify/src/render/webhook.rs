//! Webhook bodies: uniwrt's JSON (also Home Assistant's), a Discord embed, Slack blocks and a
//! Teams Adaptive Card.

use serde_json::{Value, json};
use uniwrt_core::NodeId;

use super::{color, label, truncate};
use crate::{Message, text};

/// uniwrt's JSON: the message, and each alert it tells of with what happened to it.
pub fn generic(message: &Message) -> Value {
    let alerts = message
        .items
        .iter()
        .map(|item| {
            let alert = &item.alert;
            let state = &alert.state;
            json!({
                "event": item.kind.name(),
                "id": alert.id,
                "rule": alert.rule,
                "name": state.name,
                "level": state.level,
                "subject": item.subject,
                "entity": state.subject,
                "node": state.node.map(NodeId::get),
                "client": state.client.map(|mac| mac.to_string()),
                "started_at": state.started_at.as_unix_ms(),
                "resolved_at": state.resolved_at.map(uniwrt_core::Timestamp::as_unix_ms),
                "resolution": state.resolution,
                "count": state.count,
                "silenced": state.silenced,
                "acknowledged": alert.acknowledged.is_some(),
                "cause": item.cause,
                "url": item.link,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "source": "uniwrt",
        "event": message.kind.name(),
        "title": message.title,
        "message": message.text,
        "level": message.level,
        "url": message.link,
        "at": message.at.as_unix_ms(),
        "alerts": alerts,
    })
}

/// A Discord embed; nobody is mentioned, whatever the text says.
pub(super) fn discord(message: &Message) -> Value {
    let mut embed = json!({
        "title": truncate(&message.title, 256),
        "description": truncate(&message.text, 4096),
        "color": color(message),
        "timestamp": text::rfc3339(message.at),
        "footer": { "text": format!("uniwrt · {}", label(message)) },
    });
    if let Some(link) = &message.link {
        embed["url"] = json!(link);
    }
    json!({
        "username": "uniwrt",
        "allowed_mentions": { "parse": [] },
        "embeds": [embed],
    })
}

/// A Slack message: the title (a link when it can be) and details in a coloured attachment.
pub(super) fn slack(message: &Message) -> Value {
    let title = escape_slack(&message.title);
    let title = match &message.link {
        Some(link) => format!("*<{}|{title}>*", escape_slack(link)),
        None => format!("*{title}*"),
    };
    let section = truncate(&format!("{title}\n{}", escape_slack(&message.text)), 3000);
    json!({
        "text": message.title,
        "attachments": [{
            "color": format!("#{:06x}", color(message)),
            "blocks": [
                { "type": "section", "text": { "type": "mrkdwn", "text": section } },
                { "type": "context", "elements": [
                    { "type": "mrkdwn", "text": format!("uniwrt · {}", label(message)) },
                ] },
            ],
        }],
    })
}

/// Slack's mrkdwn treats `&`, `<` and `>` as markup.
fn escape_slack(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// An Adaptive Card, as a Teams Workflows webhook ("Post to a channel when a webhook request is
/// received") takes it: a text block per line.
pub(super) fn teams(message: &Message) -> Value {
    let accent = if message.resolved() {
        "Good"
    } else {
        match message.level {
            uniwrt_model::events::EventLevel::Critical => "Attention",
            uniwrt_model::events::EventLevel::Warning => "Warning",
            _ => "Accent",
        }
    };
    let mut body = vec![json!({
        "type": "TextBlock", "text": message.title, "weight": "Bolder", "size": "Medium",
        "color": accent, "wrap": true,
    })];
    body.extend(message.text.lines().map(
        |line| json!({ "type": "TextBlock", "text": line, "wrap": true, "spacing": "Small" }),
    ));
    body.push(json!({
        "type": "TextBlock", "text": format!("uniwrt · {}", label(message)), "isSubtle": true,
        "size": "Small", "wrap": true,
    }));
    let mut card = json!({
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "type": "AdaptiveCard",
        "version": "1.4",
        "msteams": { "width": "Full" },
        "body": body,
    });
    if let Some(link) = &message.link {
        card["actions"] =
            json!([{ "type": "Action.OpenUrl", "title": "Open in uniwrt", "url": link }]);
    }
    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "contentUrl": null,
            "content": card,
        }],
    })
}

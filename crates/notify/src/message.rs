//! What a notification says: one alert that opened, reopened, is still firing or resolved
//! ([`Item`]), several of them at once (a digest), or a test; in words, with a link to the alert
//! when the controller's address is known.

use serde::Serialize;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    alerts::{AlertRecord, AlertResolution},
    events::EventLevel,
};

use crate::text::{self, Names};

/// Why a notification goes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An alert started firing.
    Opened,
    /// It fired again within its rule's cool-down.
    Reopened,
    /// It still fires, neither acknowledged nor silenced.
    Reminder,
    Resolved,
    /// A user's test of the notifier.
    Test,
    /// Several of the above at once.
    Digest,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Reopened => "reopened",
            Self::Reminder => "reminder",
            Self::Resolved => "resolved",
            Self::Test => "test",
            Self::Digest => "digest",
        }
    }
}

/// One alert to tell of.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub alert: AlertRecord,
    /// What it is about, in words.
    pub subject: String,
    /// What made it fire, in words.
    pub cause: Option<String>,
    pub link: Option<String>,
}

impl Item {
    pub fn new(kind: Kind, alert: AlertRecord, names: &Names, link_url: Option<&str>) -> Self {
        Self {
            kind,
            subject: text::subject(&alert, names),
            cause: text::cause(&alert),
            link: link_url.map(|base| format!("{base}/insights/alerts?alert={}", alert.id)),
            alert,
        }
    }

    /// `Device offline · ap-garden`, with what happened before it when it is not news.
    pub fn title(&self) -> String {
        let what = format!("{} · {}", self.alert.state.name, self.subject);
        match self.kind {
            Kind::Reopened => format!("Again: {what}"),
            Kind::Reminder => format!("Still firing: {what}"),
            Kind::Resolved => format!("Resolved: {what}"),
            _ => what,
        }
    }

    /// The details, a line each.
    pub fn lines(&self, now: Timestamp) -> Vec<String> {
        let state = &self.alert.state;
        let mut lines = Vec::new();
        lines.extend(self.cause.clone());
        let since = text::time(state.started_at);
        if let Some(resolved) = state.resolved_at {
            let after = text::duration(resolved.saturating_duration_since(state.started_at));
            let how = match state.resolution {
                Some(AlertResolution::NoData) => " (no data)",
                Some(AlertResolution::RuleChanged) => " (its rule changed)",
                Some(AlertResolution::Cleared) | None => "",
            };
            lines.push(format!("Resolved after {after}{how}, since {since}"));
        } else {
            let firing = text::duration(now.saturating_duration_since(state.started_at));
            lines.push(format!(
                "{} · firing for {firing}, since {since}",
                text::level(state.level)
            ));
        }
        if state.count > 1 {
            lines.push(format!("Fired {} times", state.count));
        }
        lines
    }
}

/// A notification, ready to render for a notifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub kind: Kind,
    /// The worst level of its alerts.
    pub level: EventLevel,
    pub title: String,
    /// The details, lines separated by `\n`.
    pub text: String,
    pub link: Option<String>,
    pub at: Timestamp,
    pub items: Vec<Item>,
}

impl Message {
    /// One alert, or a digest of several.
    pub fn new(mut items: Vec<Item>, link_url: Option<&str>, now: Timestamp) -> Self {
        if items.len() == 1
            && let Some(item) = items.pop()
        {
            return Self {
                kind: item.kind,
                level: item.alert.state.level,
                title: item.title(),
                text: item.lines(now).join("\n"),
                link: item.link.clone(),
                at: now,
                items: vec![item],
            };
        }
        let level = items
            .iter()
            .map(|item| item.alert.state.level)
            .min()
            .unwrap_or(EventLevel::Info);
        let count = |kinds: &[Kind]| items.iter().filter(|i| kinds.contains(&i.kind)).count();
        let parts = [
            (count(&[Kind::Opened, Kind::Reopened]), "firing"),
            (count(&[Kind::Reminder]), "still firing"),
            (count(&[Kind::Resolved]), "resolved"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect::<Vec<_>>();
        let lines = items
            .iter()
            .map(|item| {
                format!(
                    "• {} ({})",
                    item.title(),
                    text::level(item.alert.state.level)
                )
            })
            .collect::<Vec<_>>();
        Self {
            kind: Kind::Digest,
            level,
            title: format!("{} alerts: {}", items.len(), parts.join(", ")),
            text: lines.join("\n"),
            link: link_url.map(|base| format!("{base}/insights/alerts")),
            at: now,
            items,
        }
    }

    /// What a test sends.
    pub fn test(link_url: Option<&str>, now: Timestamp) -> Self {
        Self {
            kind: Kind::Test,
            level: EventLevel::Info,
            title: "uniwrt test notification".to_owned(),
            text: "Alerts from uniwrt reach you here.".to_owned(),
            link: link_url.map(|base| format!("{base}/insights/alerts")),
            at: now,
            items: Vec::new(),
        }
    }

    /// Whether it only tells of alerts that resolved (rendered as good news).
    pub fn resolved(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.kind == Kind::Resolved)
    }
}

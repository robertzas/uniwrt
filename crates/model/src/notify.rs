//! Notifiers (features/observability.md, Notifications): where alerts are sent, which of them,
//! and how the last delivery went. A notifier's secrets (a webhook's URL and headers, an ntfy
//! token or password, an SMTP password) are not here: they live in the vault, and the
//! `notify` crate reads them.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, NotifierId, Timestamp};

use crate::{alerts::AlertState, events::EventLevel};

/// How alerts reach people: one kind per service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotifierSpec {
    /// An ntfy topic, on ntfy.sh or a server of one's own.
    Ntfy(NtfySpec),
    /// An HTTP POST of JSON: a service's own format (Discord, Slack, Teams, Home Assistant),
    /// uniwrt's, or a template of the user's.
    Webhook(WebhookSpec),
    /// Email over SMTP, with STARTTLS or TLS.
    Email(EmailSpec),
}

impl NotifierSpec {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Ntfy(_) => "ntfy",
            Self::Webhook(_) => "webhook",
            Self::Email(_) => "email",
        }
    }
}

/// An ntfy topic. With a `username` the vault's secret is its password, else an access token
/// (if any).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NtfySpec {
    /// The server's base URL, such as `https://ntfy.sh`.
    pub server: String,
    pub topic: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// A webhook: its URL (and any headers) is a secret, since services put their tokens in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct WebhookSpec {
    pub format: WebhookFormat,
    /// The body of a `custom` webhook: JSON with `{{placeholders}}`.
    #[serde(default)]
    pub template: Option<String>,
}

/// What a webhook's body looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WebhookFormat {
    /// uniwrt's JSON: the message and its alerts.
    Generic,
    /// A Discord channel's webhook: an embed.
    Discord,
    /// A Slack incoming webhook: blocks.
    Slack,
    /// A Microsoft Teams Workflows webhook: an Adaptive Card.
    Teams,
    /// A Home Assistant webhook trigger: uniwrt's JSON (`trigger.json.title` in automations).
    HomeAssistant,
    /// The user's template.
    Custom,
}

/// Email over SMTP. With a `username` the vault's secret is its password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EmailSpec {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    #[serde(default)]
    pub username: Option<String>,
    pub from: String,
    pub to: Vec<String>,
}

/// How an SMTP connection is secured: never in the clear (D-061).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    /// Plain, then upgraded with STARTTLS (required); usually port 587.
    Starttls,
    /// TLS from the start; usually port 465.
    Tls,
}

impl SmtpSecurity {
    pub const fn default_port(self) -> u16 {
        match self {
            Self::Starttls => 587,
            Self::Tls => 465,
        }
    }
}

/// Which alerts a notifier hears of, and how often it is reminded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotifierFilter {
    /// The least level it hears of.
    pub min: EventLevel,
    /// Only alerts about these devices; every alert when empty.
    #[serde(default)]
    pub nodes: Vec<NodeId>,
    /// Also when an alert it heard of resolves.
    #[serde(default = "yes")]
    pub resolved: bool,
    /// Reminds it of an alert still firing, neither acknowledged nor silenced, this often;
    /// never when `None`.
    #[serde(default)]
    pub remind_s: Option<u32>,
}

const fn yes() -> bool {
    true
}

impl NotifierFilter {
    /// The shortest and longest reminder intervals.
    pub const MIN_REMIND_S: u32 = 15 * 60;
    pub const MAX_REMIND_S: u32 = 7 * 24 * 3600;

    /// Whether it hears of `alert`: at its level or above, about one of its devices (an alert
    /// about no device only when it names none).
    pub fn admits(&self, alert: &AlertState) -> bool {
        alert.level <= self.min
            && (self.nodes.is_empty() || alert.node.is_some_and(|n| self.nodes.contains(&n)))
    }

    pub fn reminder(&self) -> Option<Duration> {
        self.remind_s.map(|s| Duration::from_secs(s.into()))
    }
}

impl Default for NotifierFilter {
    fn default() -> Self {
        Self {
            min: EventLevel::Warning,
            nodes: Vec::new(),
            resolved: true,
            remind_s: Some(4 * 3600),
        }
    }
}

/// How a notifier's deliveries went: the latest success and the latest failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DeliveryStatus {
    pub last_ok_at: Option<Timestamp>,
    pub last_error_at: Option<Timestamp>,
    pub last_error: Option<String>,
}

/// A notifier as stored, without its secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Notifier {
    pub id: NotifierId,
    #[serde(flatten)]
    pub config: NotifierConfig,
    pub status: DeliveryStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// What users set of a notifier, its secrets aside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NotifierConfig {
    pub name: String,
    pub enabled: bool,
    pub spec: NotifierSpec,
    pub filter: NotifierFilter,
}

/// Settings every notifier shares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NotificationSettings {
    /// The controller's address as people open it (`https://uniwrt.lan:8443`): messages link
    /// to the alert there. No links when `None`.
    #[serde(default)]
    pub link_url: Option<String>,
}

impl NotificationSettings {
    /// Its key among the controller's settings.
    pub const KEY: &'static str = "notifications";
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_core::{EventId, NodeId, Timestamp};

    use super::{NotifierConfig, NotifierFilter, NotifierSpec};
    use crate::{
        alerts::{AlertCause, AlertState},
        events::EventLevel,
    };

    fn state(level: EventLevel, node: Option<NodeId>) -> AlertState {
        AlertState {
            name: "Device offline".to_owned(),
            level,
            subject: "node:3/offline".to_owned(),
            node,
            client: None,
            started_at: Timestamp::from_unix_ms(0),
            last_at: Timestamp::from_unix_ms(0),
            resolved_at: None,
            resolution: None,
            count: 1,
            silenced: false,
            cause: AlertCause::Event {
                event: EventId::new(1),
                kind: "node_offline".to_owned(),
                matches: 1,
                condition: None,
            },
        }
    }

    #[test]
    fn filters_by_level_and_device() {
        let filter = NotifierFilter::default();
        assert!(filter.admits(&state(EventLevel::Critical, None)));
        assert!(filter.admits(&state(EventLevel::Warning, Some(NodeId::new(3)))));
        assert!(!filter.admits(&state(EventLevel::Notice, None)));
        let some = NotifierFilter {
            nodes: vec![NodeId::new(3)],
            ..NotifierFilter::default()
        };
        assert!(some.admits(&state(EventLevel::Critical, Some(NodeId::new(3)))));
        assert!(!some.admits(&state(EventLevel::Critical, Some(NodeId::new(4)))));
        assert!(!some.admits(&state(EventLevel::Critical, None)));
    }

    #[test]
    fn reads_specs_tagged_by_kind() {
        let config: NotifierConfig = serde_json::from_value(json!({
            "name": "Phone",
            "enabled": true,
            "spec": { "kind": "ntfy", "server": "https://ntfy.sh", "topic": "home" },
            "filter": { "min": "warning" },
        }))
        .unwrap();
        assert!(matches!(&config.spec, NotifierSpec::Ntfy(n) if n.username.is_none()));
        assert!(config.filter.resolved);
        assert_eq!(config.filter.remind_s, None);
        let back = serde_json::to_value(&config).unwrap();
        assert_eq!(back["spec"]["kind"], "ntfy");
    }
}

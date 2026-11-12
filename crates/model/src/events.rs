//! Events (features/observability.md, Events, alerts, notifications): what happened on the
//! network, as the controller records it for its timeline. Nodes going offline and coming back,
//! reboots, WAN and mesh links going down, clients joining, leaving and roaming, annotated log
//! entries that matter, and failed jobs.
//!
//! A *condition* is a state that begins and ends, such as a node being offline: the event that
//! raises it and the one that clears it share its key ([`Condition`]), so the timeline can pair
//! them and the controller resumes them after a restart. Repeats of some events (the same log
//! annotation from the same source) fold into one record with a count.

use serde::{Deserialize, Serialize};
use uniwrt_core::{EventId, JobId, LogId, MacAddr, NodeId, RevisionId, Timestamp};

use crate::{
    client::ClientMedium,
    drift::DriftEnd,
    event::{Attachment, SteerAction, SteerReason},
    job::JobKind,
    log::Severity,
    status::StatusReason,
    wifi::{Band, ChannelWidth},
};

/// How much an event matters, most first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EventLevel {
    Critical,
    Warning,
    Notice,
    Info,
}

impl EventLevel {
    pub const ALL: [Self; 4] = [Self::Critical, Self::Warning, Self::Notice, Self::Info];

    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Critical,
            1 => Self::Warning,
            2 => Self::Notice,
            _ => Self::Info,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Notice => "notice",
            Self::Info => "info",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.name() == name)
    }

    /// A log severity as an event's level: emergency to critical are critical, errors and
    /// warnings warnings, notices notices, the rest info.
    pub const fn from_severity(severity: Severity) -> Self {
        match severity {
            Severity::Emergency | Severity::Alert | Severity::Critical => Self::Critical,
            Severity::Error | Severity::Warning => Self::Warning,
            Severity::Notice => Self::Notice,
            Severity::Info | Severity::Debug => Self::Info,
        }
    }
}

/// What an event is about, for filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    /// The controller and the nodes themselves: online, offline, reboots, flash, crashes.
    System,
    Wan,
    Mesh,
    Wifi,
    Client,
    /// LAN services: DHCP, bridges, connection tracking.
    Network,
    /// Failed logins and wrong passwords.
    Security,
}

impl EventCategory {
    pub const ALL: [Self; 7] = [
        Self::System,
        Self::Wan,
        Self::Mesh,
        Self::Wifi,
        Self::Client,
        Self::Network,
        Self::Security,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Wan => "wan",
            Self::Mesh => "mesh",
            Self::Wifi => "wifi",
            Self::Client => "client",
            Self::Network => "network",
            Self::Security => "security",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.name() == name)
    }
}

/// What happened, with what it takes to tell it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// The controller started; `version` is its own.
    ControllerStarted {
        version: String,
    },
    /// A node was adopted (in observe mode).
    NodeAdopted,
    /// A node became unreachable (raises its `offline` condition).
    NodeOffline {
        reasons: Vec<StatusReason>,
    },
    /// A node is reachable again, after `down_ms` (when the controller saw it go).
    NodeOnline {
        down_ms: Option<u64>,
    },
    /// A node's telemetry stayed missing, partial or late (raises `degraded`).
    NodeDegraded {
        reasons: Vec<StatusReason>,
    },
    NodeRecovered {
        degraded_ms: Option<u64>,
    },
    /// A node started again; `up_s` is how long it had been up before, when known.
    NodeRebooted {
        up_s: Option<u64>,
    },
    /// A node's flash (its root file system) is nearly full (raises `flash`).
    FlashLow {
        available_bytes: u64,
        total_bytes: u64,
    },
    FlashOk {
        available_bytes: u64,
        total_bytes: u64,
    },
    /// A mesh link between two adopted nodes went down: the event's node and `peer`, the
    /// topology's edge `link` (raises `mesh-down`).
    MeshLinkDown {
        link: String,
        peer: NodeId,
    },
    MeshLinkUp {
        link: String,
        peer: NodeId,
        down_ms: Option<u64>,
    },
    /// A mesh link's quality (0–100) stayed low (raises `mesh-weak`).
    MeshLinkWeak {
        link: String,
        peer: NodeId,
        quality: u8,
    },
    MeshLinkRecovered {
        link: String,
        peer: NodeId,
        quality: u8,
        weak_ms: Option<u64>,
    },
    /// A gateway's link to the internet lost its default route (raises `wan`).
    WanDown {
        device: String,
    },
    WanUp {
        device: String,
        down_ms: Option<u64>,
    },
    /// A WAN interface's addresses or delegated prefixes changed.
    WanAddressChanged {
        interface: String,
        before: Vec<String>,
        after: Vec<String>,
    },
    /// A client connected: at an access point's interface (`place`, such as `phy1-ap0`) or a
    /// port.
    ClientJoined {
        medium: ClientMedium,
        place: Option<String>,
        ssid: Option<String>,
        band: Option<Band>,
    },
    /// A client went away, after `connected_ms`.
    ClientLeft {
        connected_ms: Option<u64>,
    },
    /// A Wi-Fi client moved to another access point; `fast` over 802.11r.
    ClientRoamed {
        from: Attachment,
        to: Attachment,
        fast: bool,
    },
    /// Client steering at the event's node: usteer refused the client or kicked it at the
    /// access point `iface`, or the client answered a BSS transition request there. `to` is
    /// the access point usteer would rather have it on, or the one it said it would go to,
    /// when the controller knows it, and `to_signal_dbm` the client's signal there;
    /// `status` is the answer's 802.11 status code.
    ClientSteered {
        action: SteerAction,
        iface: String,
        reason: Option<SteerReason>,
        signal_dbm: Option<i16>,
        min_signal_dbm: Option<i16>,
        to: Option<Attachment>,
        to_signal_dbm: Option<i16>,
        status: Option<u8>,
    },
    /// A user kicked a client off the Wi-Fi, keeping it off every managed access point for
    /// `ban_secs` (0: it may come back at once).
    ClientKicked {
        ban_secs: u32,
        by: Option<String>,
    },
    /// A user blocked a client: the controller keeps it off every managed access point.
    ClientBlocked {
        by: Option<String>,
    },
    ClientUnblocked {
        by: Option<String>,
    },
    /// A device the controller had never seen joined the network.
    NewClient {
        vendor: Option<String>,
        hostname: Option<String>,
    },
    /// A radio moved to another channel; `radar` when hostapd reported radar on its node in
    /// the 2 minutes before (a DFS channel it had to leave).
    ChannelChanged {
        radio: String,
        band: Band,
        from: u16,
        to: u16,
        width: Option<ChannelWidth>,
        radar: bool,
    },
    /// A radio's channel stayed busy (raises `radio:<name>/busy`); `busy_percent` is the share
    /// of time any transmitter used it.
    ChannelCongested {
        radio: String,
        busy_percent: u8,
    },
    ChannelClear {
        radio: String,
        busy_percent: u8,
        congested_ms: Option<u64>,
    },
    /// A log entry that a pattern marks as an event (`log-patterns.toml`'s `event`): the
    /// pattern's id, the entry, its host when it is not a node's, and its text (cut short).
    LogPattern {
        pattern: String,
        log: LogId,
        host: Option<String>,
        program: Option<String>,
        text: String,
    },
    /// A job failed.
    JobFailed {
        job: JobId,
        job_kind: JobKind,
        error: Option<String>,
    },
    /// A managed package changed on its node after the controller confirmed it (drift; raises
    /// `drift:<package>`); `removed` when the file is gone.
    ConfigDrift {
        package: String,
        revision: RevisionId,
        removed: bool,
    },
    /// A drift ended: reverted, confirmed anew, acknowledged, muted or no longer watched.
    ConfigDriftEnded {
        package: String,
        how: DriftEnd,
        drift_ms: Option<u64>,
    },
    /// Autotune's weekly test of the line suggests SQM rates more than 10 % away from the
    /// site's (features/wan.md, SQM; D-076): the line got faster or slower. All in kbit/s.
    SqmRatesDrifted {
        job: JobId,
        download_kbps: u32,
        upload_kbps: u32,
        shaped_download_kbps: u32,
        shaped_upload_kbps: u32,
    },
    /// A WLAN's schedule turned it off, or on again, across the site (features/wifi.md, WLANs;
    /// D-083).
    WlanScheduled {
        wlan: String,
        ssid: String,
        on: bool,
    },
    /// The controller could not turn a node's BSSes on `phy` off or on as the schedules want
    /// (raises `wlan-schedule:<phy>`, cleared once it could).
    WlanScheduleFailed {
        phy: String,
        error: String,
    },
    WlanScheduleOk {
        phy: String,
    },
    /// The controller could not keep the blocked and kicked clients off a node's access points
    /// (raises `client-bans`, cleared once it could; features/wifi.md, Client actions).
    ClientBansFailed {
        error: String,
    },
    ClientBansOk,
    /// The controller could not give a node's access points their 802.11k neighbor lists
    /// (raises `neighbor-reports`, cleared once it could; features/wifi.md, Roaming).
    NeighborReportsFailed {
        error: String,
    },
    NeighborReportsOk,
}

impl EventKind {
    /// Its usual category and level; a [`LogPattern`](Self::LogPattern)'s come from the pattern.
    pub const fn defaults(&self) -> (EventCategory, EventLevel) {
        use EventCategory as C;
        use EventLevel as L;
        match self {
            Self::NodeOffline { .. } => (C::System, L::Critical),
            Self::NodeDegraded { .. }
            | Self::FlashLow { .. }
            | Self::NodeRebooted { .. }
            | Self::LogPattern { .. }
            | Self::JobFailed { .. }
            | Self::ConfigDrift { .. } => (C::System, L::Warning),
            Self::ControllerStarted { .. }
            | Self::NodeAdopted
            | Self::NodeOnline { .. }
            | Self::NodeRecovered { .. }
            | Self::FlashOk { .. }
            | Self::ConfigDriftEnded { .. } => (C::System, L::Notice),
            Self::MeshLinkDown { .. } => (C::Mesh, L::Critical),
            Self::MeshLinkWeak { .. } => (C::Mesh, L::Warning),
            Self::MeshLinkUp { .. } | Self::MeshLinkRecovered { .. } => (C::Mesh, L::Notice),
            Self::WanDown { .. } => (C::Wan, L::Critical),
            Self::WanUp { .. } | Self::WanAddressChanged { .. } | Self::SqmRatesDrifted { .. } => {
                (C::Wan, L::Notice)
            }
            Self::ClientJoined { .. } | Self::ClientLeft { .. } | Self::ClientRoamed { .. } => {
                (C::Client, L::Info)
            }
            Self::ClientSteered { action, .. } => {
                let kicked = matches!(action, SteerAction::SignalKick | SteerAction::LoadKick);
                (C::Client, if kicked { L::Notice } else { L::Info })
            }
            Self::NewClient { .. }
            | Self::ClientKicked { .. }
            | Self::ClientBlocked { .. }
            | Self::ClientUnblocked { .. } => (C::Client, L::Notice),
            Self::ChannelChanged { radar: true, .. }
            | Self::ChannelCongested { .. }
            | Self::WlanScheduleFailed { .. }
            | Self::ClientBansFailed { .. }
            | Self::NeighborReportsFailed { .. } => (C::Wifi, L::Warning),
            Self::ChannelChanged { .. }
            | Self::ChannelClear { .. }
            | Self::WlanScheduleOk { .. }
            | Self::ClientBansOk
            | Self::NeighborReportsOk => (C::Wifi, L::Notice),
            Self::WlanScheduled { .. } => (C::Wifi, L::Info),
        }
    }

    /// The name it is stored and filtered under: its serde tag.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ControllerStarted { .. } => "controller_started",
            Self::NodeAdopted => "node_adopted",
            Self::NodeOffline { .. } => "node_offline",
            Self::NodeOnline { .. } => "node_online",
            Self::NodeDegraded { .. } => "node_degraded",
            Self::NodeRecovered { .. } => "node_recovered",
            Self::NodeRebooted { .. } => "node_rebooted",
            Self::FlashLow { .. } => "flash_low",
            Self::FlashOk { .. } => "flash_ok",
            Self::MeshLinkDown { .. } => "mesh_link_down",
            Self::MeshLinkUp { .. } => "mesh_link_up",
            Self::MeshLinkWeak { .. } => "mesh_link_weak",
            Self::MeshLinkRecovered { .. } => "mesh_link_recovered",
            Self::WanDown { .. } => "wan_down",
            Self::WanUp { .. } => "wan_up",
            Self::WanAddressChanged { .. } => "wan_address_changed",
            Self::ClientJoined { .. } => "client_joined",
            Self::ClientLeft { .. } => "client_left",
            Self::ClientRoamed { .. } => "client_roamed",
            Self::ClientSteered { .. } => "client_steered",
            Self::NewClient { .. } => "new_client",
            Self::ChannelChanged { .. } => "channel_changed",
            Self::ChannelCongested { .. } => "channel_congested",
            Self::ChannelClear { .. } => "channel_clear",
            Self::LogPattern { .. } => "log_pattern",
            Self::JobFailed { .. } => "job_failed",
            Self::ConfigDrift { .. } => "config_drift",
            Self::ConfigDriftEnded { .. } => "config_drift_ended",
            Self::SqmRatesDrifted { .. } => "sqm_rates_drifted",
            Self::WlanScheduled { .. } => "wlan_scheduled",
            Self::WlanScheduleFailed { .. } => "wlan_schedule_failed",
            Self::WlanScheduleOk { .. } => "wlan_schedule_ok",
            Self::ClientKicked { .. } => "client_kicked",
            Self::ClientBlocked { .. } => "client_blocked",
            Self::ClientUnblocked { .. } => "client_unblocked",
            Self::ClientBansFailed { .. } => "client_bans_failed",
            Self::ClientBansOk => "client_bans_ok",
            Self::NeighborReportsFailed { .. } => "neighbor_reports_failed",
            Self::NeighborReportsOk => "neighbor_reports_ok",
        }
    }
}

/// The condition an event raises or clears: its key (such as `node:3/offline`), and which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Condition {
    pub key: String,
    pub raised: bool,
}

/// An event about to be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEvent {
    /// When it happened, on the controller's clock (a reboot's, when the node started).
    pub at: Timestamp,
    /// When it last happened, for folded repeats; `at` otherwise.
    pub last_at: Timestamp,
    pub count: u32,
    pub level: EventLevel,
    pub category: EventCategory,
    pub node: Option<NodeId>,
    pub client: Option<MacAddr>,
    pub condition: Option<Condition>,
    pub what: EventKind,
    /// Repeats with the same key fold into one record (not stored).
    pub fold: Option<String>,
}

impl NewEvent {
    /// `what` happened at `at`, with its usual category and level.
    pub fn new(at: Timestamp, what: EventKind) -> Self {
        let (category, level) = what.defaults();
        Self {
            at,
            last_at: at,
            count: 1,
            level,
            category,
            node: None,
            client: None,
            condition: None,
            what,
            fold: None,
        }
    }

    #[must_use]
    pub const fn node(mut self, node: NodeId) -> Self {
        self.node = Some(node);
        self
    }

    #[must_use]
    pub const fn client(mut self, mac: MacAddr) -> Self {
        self.client = Some(mac);
        self
    }

    #[must_use]
    pub fn condition(mut self, key: impl Into<String>, raised: bool) -> Self {
        self.condition = Some(Condition {
            key: key.into(),
            raised,
        });
        self
    }

    #[must_use]
    pub fn fold(mut self, key: impl Into<String>) -> Self {
        self.fold = Some(key.into());
        self
    }
}

/// What to store for one event: a new record, or more repeats of one recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventWrite {
    Insert(NewEvent),
    /// `count` more repeats of the recorded event `id`, the last at `last_at`.
    Fold {
        id: EventId,
        last_at: Timestamp,
        count: u32,
    },
}

/// An event as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EventRecord {
    pub id: EventId,
    pub at: Timestamp,
    /// When it last happened: later than `at` once repeats folded into it.
    pub last_at: Timestamp,
    /// How many times it happened.
    pub count: u32,
    pub level: EventLevel,
    pub category: EventCategory,
    /// The node it happened on.
    pub node: Option<NodeId>,
    /// The client it is about.
    pub client: Option<MacAddr>,
    pub condition: Option<Condition>,
    pub what: EventKind,
}

impl EventRecord {
    pub fn new(id: EventId, event: NewEvent) -> Self {
        Self {
            id,
            at: event.at,
            last_at: event.last_at,
            count: event.count,
            level: event.level,
            category: event.category,
            node: event.node,
            client: event.client,
            condition: event.condition,
            what: event.what,
        }
    }
}

/// Which events to read: each part narrows, and an empty list or `None` keeps every event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct EventFilter {
    pub categories: Vec<EventCategory>,
    /// The least that matters to include: `warning` keeps warnings and critical ones.
    pub min: Option<EventLevel>,
    pub nodes: Vec<NodeId>,
    pub clients: Vec<MacAddr>,
    /// Happened at or after `from`, and before `to`.
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

impl EventFilter {
    pub fn admits(&self, event: &EventRecord) -> bool {
        (self.categories.is_empty() || self.categories.contains(&event.category))
            && self.min.is_none_or(|min| event.level <= min)
            && (self.nodes.is_empty() || event.node.is_some_and(|n| self.nodes.contains(&n)))
            && (self.clients.is_empty() || event.client.is_some_and(|c| self.clients.contains(&c)))
            && self.from.is_none_or(|from| event.at >= from)
            && self.to.is_none_or(|to| event.at < to)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_core::{EventId, NodeId, Timestamp};

    use super::{EventCategory, EventFilter, EventKind, EventLevel, EventRecord, NewEvent};
    use crate::log::Severity;

    fn record(what: EventKind) -> EventRecord {
        EventRecord::new(
            EventId::new(1),
            NewEvent::new(Timestamp::from_unix_ms(1_000), what).node(NodeId::new(3)),
        )
    }

    #[test]
    fn serializes_with_the_kind_as_a_tag() {
        let event = record(EventKind::WanDown {
            device: "eth1".to_owned(),
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "id": 1, "at": 1000, "last_at": 1000, "count": 1, "level": "critical",
                "category": "wan", "node": 3, "client": null, "condition": null,
                "what": { "kind": "wan_down", "device": "eth1" },
            })
        );
        let back: EventRecord = serde_json::from_value(serde_json::to_value(&event).unwrap())
            .expect("an event reads back");
        assert_eq!(back, event);
    }

    #[test]
    fn names_match_the_serde_tags() {
        for what in [
            EventKind::NodeAdopted,
            EventKind::NodeOnline { down_ms: None },
            EventKind::ClientLeft { connected_ms: None },
            EventKind::NewClient {
                vendor: None,
                hostname: None,
            },
        ] {
            let value = serde_json::to_value(&what).unwrap();
            assert_eq!(value["kind"], what.name());
        }
    }

    #[test]
    fn levels_and_categories_parse_their_names() {
        for level in EventLevel::ALL {
            assert_eq!(EventLevel::parse(level.name()), Some(level));
            assert_eq!(EventLevel::from_code(level.code()), level);
            assert_eq!(serde_json::to_value(level).unwrap(), level.name());
        }
        for category in EventCategory::ALL {
            assert_eq!(EventCategory::parse(category.name()), Some(category));
            assert_eq!(serde_json::to_value(category).unwrap(), category.name());
        }
        assert_eq!(
            EventLevel::from_severity(Severity::Error),
            EventLevel::Warning
        );
        assert_eq!(EventLevel::from_severity(Severity::Debug), EventLevel::Info);
    }

    #[test]
    fn filters_narrow() {
        let event = record(EventKind::NodeOffline {
            reasons: Vec::new(),
        });
        assert!(EventFilter::default().admits(&event));
        let filter = |f: EventFilter| f.admits(&event);
        assert!(filter(EventFilter {
            min: Some(EventLevel::Warning),
            ..EventFilter::default()
        }));
        assert!(!filter(EventFilter {
            categories: vec![EventCategory::Wan],
            ..EventFilter::default()
        }));
        assert!(!filter(EventFilter {
            nodes: vec![NodeId::new(4)],
            ..EventFilter::default()
        }));
        assert!(!filter(EventFilter {
            clients: vec!["f0:18:98:00:00:01".parse().unwrap()],
            ..EventFilter::default()
        }));
        assert!(!filter(EventFilter {
            to: Some(Timestamp::from_unix_ms(1_000)),
            ..EventFilter::default()
        }));
    }
}

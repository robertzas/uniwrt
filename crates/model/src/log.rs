//! Log entries (observability.md, Logs): what nodes log, read through the node agent or received
//! by syslog, and what other hosts send by syslog; stored as [`LogRecord`]s, found with a
//! [`LogFilter`], counted in [`LogBucket`]s, explained by [`LogPattern`]s, and kept as
//! [`SavedSearch`]es.

use serde::{Deserialize, Serialize};
use uniwrt_core::{LogId, NodeId, SearchId, Timestamp, UserId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogEntry {
    /// logd's sequence number, when the entry was read from logd. It starts again at 0 when
    /// logd restarts.
    pub seq: Option<u64>,
    /// When it was logged, on the node's clock; when it arrived, for a syslog message whose
    /// time names no zone (RFC 3164).
    pub time: Timestamp,
    /// The syslog facility's code (RFC 5424): 0 kern, 1 user, 3 daemon, 4 auth, 10 authpriv,
    /// 16–23 local0–local7…
    pub facility: u8,
    pub severity: Severity,
    /// The program that logged it, such as `hostapd`, or `kernel` for the kernel's messages,
    /// when the message names one.
    pub program: Option<String>,
    pub pid: Option<u32>,
    /// The text after the program's tag.
    pub message: String,
}

/// Syslog severities, most severe first. They order as in syslog, so `severity <=
/// Severity::Warning` is a warning or worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Emergency,
    Alert,
    Critical,
    Error,
    Warning,
    Notice,
    Info,
    Debug,
}

impl Severity {
    pub const ALL: [Self; 8] = [
        Self::Emergency,
        Self::Alert,
        Self::Critical,
        Self::Error,
        Self::Warning,
        Self::Notice,
        Self::Info,
        Self::Debug,
    ];

    /// The severity of a syslog priority's low three bits.
    pub const fn from_code(code: u8) -> Self {
        match code & 7 {
            0 => Self::Emergency,
            1 => Self::Alert,
            2 => Self::Critical,
            3 => Self::Error,
            4 => Self::Warning,
            5 => Self::Notice,
            6 => Self::Info,
            _ => Self::Debug,
        }
    }

    /// Its syslog code, 0 (emergency) to 7 (debug).
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Its name as serde writes it.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Emergency => "emergency",
            Self::Alert => "alert",
            Self::Critical => "critical",
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Notice => "notice",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }

    /// The severity a name gives, as serde writes it or syslog abbreviates it (`err`, `warn`,
    /// `crit`, `emerg`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "emergency" | "emerg" | "panic" => Self::Emergency,
            "alert" => Self::Alert,
            "critical" | "crit" => Self::Critical,
            "error" | "err" => Self::Error,
            "warning" | "warn" => Self::Warning,
            "notice" => Self::Notice,
            "info" => Self::Info,
            "debug" => Self::Debug,
            _ => return None,
        })
    }
}

/// Splits a syslog message's tag from its text: `dnsmasq-dhcp[2092]: DHCPACK…` →
/// (`dnsmasq-dhcp`, 2092, `DHCPACK…`). A message without a tag is all text.
pub fn split_tag(msg: &str) -> (Option<String>, Option<u32>, String) {
    let end = msg
        .find(|c: char| c == ':' || c == '[' || c.is_whitespace())
        .unwrap_or(msg.len());
    let (name, rest) = msg.split_at(end);
    let (pid, rest) = match rest.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((pid, rest)) => match pid.parse() {
                Ok(pid) => (Some(pid), rest),
                Err(_) => return (None, None, msg.to_owned()),
            },
            None => return (None, None, msg.to_owned()),
        },
        None => (None, rest),
    };
    match rest.strip_prefix(':') {
        Some(text) if !name.is_empty() && (text.is_empty() || text.starts_with(' ')) => {
            (Some(name.to_owned()), pid, text.trim_start().to_owned())
        }
        _ => (None, None, msg.to_owned()),
    }
}

/// How an entry reached the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LogVia {
    /// The node agent read it from logd.
    Agent,
    /// It came by syslog (UDP or TCP).
    Syslog,
}

/// An entry to store: where it came from, and the pattern that annotates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLog {
    /// When it arrived, on the controller's clock.
    pub received: Timestamp,
    /// The adopted node that logged it; `None` for another host.
    pub node: Option<NodeId>,
    /// Another host's name, or its address when it gave none; `None` for a node.
    pub host: Option<String>,
    pub via: LogVia,
    pub entry: LogEntry,
    /// The id of the `log-patterns.toml` pattern it matches.
    pub pattern: Option<String>,
}

/// A stored entry, as the API and the WS topic `logs` carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogRecord {
    pub id: LogId,
    pub received: Timestamp,
    pub node: Option<NodeId>,
    pub host: Option<String>,
    pub via: LogVia,
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub entry: LogEntry,
    pub pattern: Option<String>,
}

impl LogRecord {
    pub fn new(id: LogId, log: NewLog) -> Self {
        Self {
            id,
            received: log.received,
            node: log.node,
            host: log.host,
            via: log.via,
            entry: log.entry,
            pattern: log.pattern,
        }
    }
}

/// Which entries to find. Empty lists and `None`s don't filter; entries match every filter
/// given, and any one of a list's values. `nodes` and `hosts` name the sources together: an
/// entry from any node or host named passes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct LogFilter {
    pub nodes: Vec<NodeId>,
    pub hosts: Vec<String>,
    pub programs: Vec<String>,
    /// The least severe to include: `warning` keeps warnings and worse.
    pub min: Option<Severity>,
    /// Words to find (full-text): every word must appear, `"a phrase"` as written, a word ending
    /// in `*` as a prefix, and a word starting with `-` must not appear.
    pub text: Option<String>,
    /// Only annotated entries (a `log-patterns.toml` pattern matched them).
    pub annotated: bool,
    /// Logged at or after `from` and before `to`.
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
}

impl LogFilter {
    /// Whether `record` passes the filter, but for `text`, which only the store's full-text
    /// index answers.
    pub fn admits(&self, record: &LogRecord) -> bool {
        let entry = &record.entry;
        ((self.nodes.is_empty() && self.hosts.is_empty())
            || record.node.is_some_and(|node| self.nodes.contains(&node))
            || record
                .host
                .as_ref()
                .is_some_and(|host| self.hosts.contains(host)))
            && (self.programs.is_empty()
                || entry
                    .program
                    .as_ref()
                    .is_some_and(|p| self.programs.contains(p)))
            && self.min.is_none_or(|min| entry.severity <= min)
            && (!self.annotated || record.pattern.is_some())
            && self.from.is_none_or(|from| entry.time >= from)
            && self.to.is_none_or(|to| entry.time < to)
    }
}

/// How many entries a span of time holds, by severity: `counts[severity.code()]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogBucket {
    pub start: Timestamp,
    pub counts: [u32; 8],
}

/// Who has logged lately: the hosts other than nodes, and the programs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogSources {
    pub hosts: Vec<String>,
    pub programs: Vec<String>,
}

/// What a `log-patterns.toml` pattern says about the entries it matches (the `syslog` crate's
/// annotations).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LogPattern {
    pub id: String,
    /// The programs whose entries it applies to; every program when empty.
    pub programs: Vec<String>,
    /// How much it matters, whatever the entry's own severity.
    pub severity: Severity,
    /// What happened, for people.
    pub meaning: String,
    /// What to check or do about it.
    pub hint: Option<String>,
    /// Where to read more.
    pub link: Option<String>,
}

/// A search users saved, shared by all of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SavedSearch {
    pub id: SearchId,
    pub name: String,
    pub filter: LogFilter,
    pub created_by: Option<UserId>,
    pub created_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{LogId, NodeId, Timestamp};

    use super::{LogEntry, LogFilter, LogRecord, LogVia, Severity, split_tag};

    #[test]
    fn codes_map_to_severities_in_order() {
        let all: Vec<_> = (0..8).map(Severity::from_code).collect();
        assert_eq!(all, Severity::ALL);
        assert!(all.is_sorted() && all[0] == Severity::Emergency && all[7] == Severity::Debug);
        // A priority's facility bits do not matter: 29 is daemon.notice.
        assert_eq!(Severity::from_code(29), Severity::Notice);
        assert_eq!(serde_json::to_value(Severity::Warning).unwrap(), "warning");
        for severity in Severity::ALL {
            assert_eq!(Severity::from_code(severity.code()), severity);
            assert_eq!(Severity::parse(severity.name()), Some(severity));
            assert_eq!(serde_json::to_value(severity).unwrap(), severity.name());
        }
        assert_eq!(Severity::parse("err"), Some(Severity::Error));
        assert_eq!(Severity::parse("loud"), None);
    }

    #[test]
    fn tags_split_into_program_pid_and_text() {
        let tag = |msg| split_tag(msg);
        assert_eq!(
            tag("dnsmasq-dhcp[2092]: DHCPACK(br-lan)"),
            (
                Some("dnsmasq-dhcp".into()),
                Some(2092),
                "DHCPACK(br-lan)".into()
            )
        );
        assert_eq!(
            tag("netifd: up"),
            (Some("netifd".into()), None, "up".into())
        );
        assert_eq!(tag("kernel:"), (Some("kernel".into()), None, String::new()));
        for untagged in ["no tag here", "a[x]: b", "a[1: b", "url:http", ": x", ""] {
            assert_eq!(tag(untagged), (None, None, untagged.to_owned()));
        }
    }

    #[test]
    fn filters_admit_what_matches_every_part() {
        let record = LogRecord {
            id: LogId::new(1),
            received: Timestamp::from_unix_ms(2_000),
            node: Some(NodeId::new(3)),
            host: None,
            via: LogVia::Agent,
            entry: LogEntry {
                seq: None,
                time: Timestamp::from_unix_ms(1_000),
                facility: 3,
                severity: Severity::Warning,
                program: Some("hostapd".into()),
                pid: None,
                message: "x".into(),
            },
            pattern: None,
        };
        let admits = |filter: LogFilter| filter.admits(&record);
        assert!(admits(LogFilter::default()));
        assert!(admits(LogFilter {
            nodes: vec![NodeId::new(3)],
            programs: vec!["hostapd".into()],
            min: Some(Severity::Warning),
            from: Some(Timestamp::from_unix_ms(1_000)),
            to: Some(Timestamp::from_unix_ms(1_001)),
            ..LogFilter::default()
        }));
        for filter in [
            LogFilter {
                nodes: vec![NodeId::new(4)],
                ..LogFilter::default()
            },
            LogFilter {
                hosts: vec!["sw".into()],
                ..LogFilter::default()
            },
            LogFilter {
                programs: vec!["netifd".into()],
                ..LogFilter::default()
            },
            LogFilter {
                min: Some(Severity::Error),
                ..LogFilter::default()
            },
            LogFilter {
                annotated: true,
                ..LogFilter::default()
            },
            LogFilter {
                to: Some(Timestamp::from_unix_ms(1_000)),
                ..LogFilter::default()
            },
        ] {
            assert!(!admits(filter));
        }
        // Stored and read back as JSON, with the entry's fields flattened.
        let json = serde_json::to_value(&record).unwrap();
        assert_eq!(json["program"], "hostapd");
        assert_eq!(serde_json::from_value::<LogRecord>(json).unwrap(), record);
    }
}

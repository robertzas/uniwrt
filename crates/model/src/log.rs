//! Log entries from nodes (observability.md, Logs): what logd keeps, read through the node agent
//! for now, and from remote syslog later (M2.5).

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// logd's sequence number, when the entry was read from logd. It starts again at 0 when
    /// logd restarts.
    pub seq: Option<u64>,
    /// When it was logged, on the node's clock.
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
}

#[cfg(test)]
mod tests {
    use super::Severity;

    #[test]
    fn codes_map_to_severities_in_order() {
        let all: Vec<_> = (0..8).map(Severity::from_code).collect();
        assert!(all.is_sorted() && all[0] == Severity::Emergency && all[7] == Severity::Debug);
        // A priority's facility bits do not matter: 29 is daemon.notice.
        assert_eq!(Severity::from_code(29), Severity::Notice);
        assert_eq!(serde_json::to_value(Severity::Warning).unwrap(), "warning");
    }
}

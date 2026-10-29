//! A node's status as the engine derives it from its connection and its telemetry: online,
//! degraded or offline, with the reasons.

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub state: NodeState,
    /// When `state` last changed, on the controller's clock.
    pub since: Timestamp,
    /// Why the node is not online, most significant first; empty when it is.
    pub reasons: Vec<StatusReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Connected, with its agent running and its telemetry arriving.
    Online,
    /// Reachable, but its telemetry is missing, partial or late.
    Degraded,
    /// Not reachable.
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatusReason {
    /// Connecting, the `attempt`-th time since the node was last up.
    Connecting {
        attempt: u32,
    },
    /// The last connection attempt failed; the next one is at `retry_at`.
    Unreachable {
        error: String,
        retry_at: Timestamp,
    },
    /// The node presented another SSH host key than the pinned one (both SHA-256
    /// fingerprints). It stays offline until an admin re-trusts it.
    HostKeyMismatch {
        presented: String,
        pinned: String,
    },
    /// The controller no longer watches the node.
    Stopped,
    /// The connection runs no agent: commands only.
    NoAgent,
    AgentStarting,
    /// The agent is not running; commands still work. It starts again at `retry_at`.
    AgentFailed {
        error: String,
        retry_at: Timestamp,
    },
    /// A request to the agent failed, such as `set_interval` (telemetry is then polled) or
    /// `collect` (retried at the next interval).
    RequestFailed {
        op: String,
        error: String,
    },
    /// No telemetry has arrived since `last` (never, when `None`) for longer than its
    /// intervals allow.
    TelemetryStale {
        last: Option<Timestamp>,
    },
    /// A collect set fails, so that part of the facts is out of date.
    CollectFailed {
        set: String,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uniwrt_core::Timestamp;

    use super::{NodeState, NodeStatus, StatusReason};

    #[test]
    fn reasons_are_tagged_by_kind() {
        let status = NodeStatus {
            state: NodeState::Degraded,
            since: Timestamp::from_unix_ms(1_000),
            reasons: vec![
                StatusReason::AgentStarting,
                StatusReason::CollectFailed {
                    set: "batman".to_owned(),
                    error: "batctl is not installed".to_owned(),
                },
            ],
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(
            value,
            json!({
                "state": "degraded", "since": 1000,
                "reasons": [
                    {"kind": "agent_starting"},
                    {"kind": "collect_failed", "set": "batman", "error": "batctl is not installed"}
                ]
            })
        );
        assert_eq!(serde_json::from_value::<NodeStatus>(value).unwrap(), status);
    }
}

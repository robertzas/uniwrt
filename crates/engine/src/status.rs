//! A node's status (`model::status`) from its link's state and the health of its telemetry.

use std::collections::BTreeMap;

use uniwrt_core::Timestamp;
use uniwrt_model::status::{NodeState, NodeStatus, StatusReason};
use uniwrt_node::{AgentState, LinkState, proto::ops::Set};

/// What went wrong with the telemetry of the running agent; reset when another one starts.
#[derive(Debug, Default)]
pub(crate) struct Health {
    /// Requests that failed, by op, until one succeeds.
    pub(crate) requests: BTreeMap<&'static str, String>,
    /// Sets that failed on their latest collection.
    pub(crate) sets: BTreeMap<Set, String>,
    /// Telemetry is late.
    pub(crate) stale: bool,
    /// When telemetry last arrived.
    pub(crate) last_update: Option<Timestamp>,
}

impl Health {
    /// Forgets the problems of the agent before; when telemetry last arrived still holds.
    pub(crate) fn reset(&mut self) {
        self.requests.clear();
        self.sets.clear();
        self.stale = false;
    }
}

/// The state and the reasons for it.
pub(crate) fn derive(link: &LinkState, health: &Health) -> (NodeState, Vec<StatusReason>) {
    let offline = |reason| (NodeState::Offline, vec![reason]);
    let degraded = |reason| (NodeState::Degraded, vec![reason]);
    match link {
        LinkState::Connecting { attempt } => {
            offline(StatusReason::Connecting { attempt: *attempt })
        }
        LinkState::Down { error, retry_at } => offline(StatusReason::Unreachable {
            error: error.clone(),
            retry_at: *retry_at,
        }),
        LinkState::Quarantined { presented, pinned } => offline(StatusReason::HostKeyMismatch {
            presented: presented.fingerprint().to_owned(),
            pinned: pinned.fingerprint().to_owned(),
        }),
        LinkState::Stopped => offline(StatusReason::Stopped),
        LinkState::Up { agent, .. } => match agent {
            AgentState::None => degraded(StatusReason::NoAgent),
            AgentState::Starting => degraded(StatusReason::AgentStarting),
            AgentState::Failed { error, retry_at } => degraded(StatusReason::AgentFailed {
                error: error.clone(),
                retry_at: *retry_at,
            }),
            AgentState::Running(_) => running(health),
        },
    }
}

fn running(health: &Health) -> (NodeState, Vec<StatusReason>) {
    let mut reasons: Vec<_> = health
        .requests
        .iter()
        .map(|(op, error)| StatusReason::RequestFailed {
            op: (*op).to_owned(),
            error: error.clone(),
        })
        .collect();
    if health.stale {
        reasons.push(StatusReason::TelemetryStale {
            last: health.last_update,
        });
    }
    reasons.extend(
        health
            .sets
            .iter()
            .map(|(set, error)| StatusReason::CollectFailed {
                set: set.as_str().to_owned(),
                error: error.clone(),
            }),
    );
    let state = if reasons.is_empty() {
        NodeState::Online
    } else {
        NodeState::Degraded
    };
    (state, reasons)
}

/// The latest status, with when its state began.
#[derive(Debug, Default)]
pub(crate) struct Tracker {
    current: Option<NodeStatus>,
}

impl Tracker {
    /// The new status if it differs from the latest one; `since` moves only with the state.
    pub(crate) fn update(
        &mut self,
        now: Timestamp,
        (state, reasons): (NodeState, Vec<StatusReason>),
    ) -> Option<NodeStatus> {
        let since = match &self.current {
            Some(current) if current.state == state && current.reasons == reasons => return None,
            Some(current) if current.state == state => current.since,
            _ => now,
        };
        let status = NodeStatus {
            state,
            since,
            reasons,
        };
        self.current = Some(status.clone());
        Some(status)
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;
    use uniwrt_model::status::{NodeState, StatusReason};
    use uniwrt_node::{
        AgentState, ControllerKey, Hello, HostKey, LinkState,
        proto::{AgentCaps, ops::Set},
    };

    use super::{Health, Tracker, derive};

    fn host_key() -> HostKey {
        HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap()).unwrap()
    }

    fn up(agent: AgentState) -> LinkState {
        LinkState::Up {
            since: Timestamp::UNIX_EPOCH,
            agent,
        }
    }

    fn running() -> LinkState {
        up(AgentState::Running(Hello {
            proto: 1,
            agent: "hash".to_owned(),
            release: None,
            caps: AgentCaps::default(),
        }))
    }

    #[test]
    fn link_states_map_to_offline_and_degraded() {
        let health = Health::default();
        let at = Timestamp::from_unix_ms(5);
        let cases = [
            (
                LinkState::Connecting { attempt: 2 },
                NodeState::Offline,
                StatusReason::Connecting { attempt: 2 },
            ),
            (
                LinkState::Down {
                    error: "refused".to_owned(),
                    retry_at: at,
                },
                NodeState::Offline,
                StatusReason::Unreachable {
                    error: "refused".to_owned(),
                    retry_at: at,
                },
            ),
            (
                LinkState::Stopped,
                NodeState::Offline,
                StatusReason::Stopped,
            ),
            (
                up(AgentState::None),
                NodeState::Degraded,
                StatusReason::NoAgent,
            ),
            (
                up(AgentState::Starting),
                NodeState::Degraded,
                StatusReason::AgentStarting,
            ),
            (
                up(AgentState::Failed {
                    error: "no ucode".to_owned(),
                    retry_at: at,
                }),
                NodeState::Degraded,
                StatusReason::AgentFailed {
                    error: "no ucode".to_owned(),
                    retry_at: at,
                },
            ),
        ];
        for (link, state, reason) in cases {
            assert_eq!(derive(&link, &health), (state, vec![reason]), "{link:?}");
        }
        let (presented, pinned) = (host_key(), host_key());
        let quarantined = LinkState::Quarantined {
            presented: presented.clone(),
            pinned: pinned.clone(),
        };
        assert_eq!(
            derive(&quarantined, &health),
            (
                NodeState::Offline,
                vec![StatusReason::HostKeyMismatch {
                    presented: presented.fingerprint().to_owned(),
                    pinned: pinned.fingerprint().to_owned(),
                }]
            )
        );
    }

    #[test]
    fn a_running_agent_is_online_unless_its_telemetry_is_unhealthy() {
        let mut health = Health::default();
        assert_eq!(derive(&running(), &health), (NodeState::Online, vec![]));
        health
            .sets
            .insert(Set::Batman, "batctl is not installed".to_owned());
        health.stale = true;
        health.requests.insert("set_interval", "timeout".to_owned());
        let (state, reasons) = derive(&running(), &health);
        assert_eq!(state, NodeState::Degraded);
        assert_eq!(
            reasons,
            [
                StatusReason::RequestFailed {
                    op: "set_interval".to_owned(),
                    error: "timeout".to_owned()
                },
                StatusReason::TelemetryStale { last: None },
                StatusReason::CollectFailed {
                    set: "batman".to_owned(),
                    error: "batctl is not installed".to_owned()
                },
            ]
        );
        // A link that is not up says why; the agent's health does not matter then.
        assert_eq!(
            derive(&LinkState::Stopped, &health).1,
            [StatusReason::Stopped]
        );
        health.reset();
        assert_eq!(derive(&running(), &health).0, NodeState::Online);
    }

    #[test]
    fn the_tracker_reports_changes_and_keeps_since_while_the_state_holds() {
        let mut tracker = Tracker::default();
        let t = Timestamp::from_unix_ms;
        let connecting = |attempt| {
            (
                NodeState::Offline,
                vec![StatusReason::Connecting { attempt }],
            )
        };
        assert_eq!(tracker.update(t(1), connecting(1)).unwrap().since, t(1));
        assert_eq!(tracker.update(t(2), connecting(1)), None);
        let second = tracker.update(t(3), connecting(2)).unwrap();
        assert_eq!((second.since, second.reasons.len()), (t(1), 1));
        let online = tracker.update(t(4), (NodeState::Online, vec![])).unwrap();
        assert_eq!(online.since, t(4));
    }
}

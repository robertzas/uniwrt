//! The engine's settings. The defaults follow features/observability.md (Telemetry pipeline) and
//! D-037.

use std::time::Duration;

use uniwrt_node::proto::ops::LogMode;

use crate::Intervals;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub intervals: Intervals,
    /// How a node's log is followed while someone follows it
    /// ([`Engine::follow_logs`](crate::Engine::follow_logs)).
    pub log_mode: LogMode,
    /// Entries a follow starts with. After the agent restarts, the follow starts again with
    /// them, so what was logged meanwhile is not lost; entries already delivered are skipped.
    pub log_backfill: u32,
    /// How long after a pushed event the set it touches is collected again (a station event:
    /// `stations`); the events in that time share one collect.
    pub refresh_delay: Duration,
    /// How long after leaving an access point a client's join elsewhere is still a roam.
    pub roam_window: Duration,
    /// How long a request to an agent may take.
    pub request_timeout: Duration,
    /// How long after a node's facts or status change the topology is built again; the changes
    /// of that moment share one build.
    pub topology_delay: Duration,
    /// Events the bus buffers for a subscriber that falls behind; it then skips ahead.
    pub bus_capacity: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            intervals: Intervals::default(),
            log_mode: LogMode::Poll,
            log_backfill: 200,
            refresh_delay: Duration::from_millis(500),
            roam_window: Duration::from_secs(15),
            request_timeout: Duration::from_secs(30),
            topology_delay: Duration::from_secs(1),
            bus_capacity: 4096,
        }
    }
}

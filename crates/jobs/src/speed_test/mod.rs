//! The speed test (features/wan.md, SQM; D-076): measures the line from the gateway with its
//! own tools (`node-agent/speedtest.sh`: uclient-fetch, ping, the WAN device's counters), so
//! nothing needs installing and observe mode is fine.
//!
//! Steps: connect (the gateway) → idle (round trips with the line quiet) → download (four
//! HTTPS streams for 12 s, pinging once a second) → upload (the same, posting) → record (the
//! result in the history). The rate leaves the first 2 s out (slow start); latency under load
//! is the 90th percentile, a lost reply counting as 1 s. With `bypass_sqm`, SQM is stopped
//! during each load phase and started again as it ends, whatever ends it (the script's trap),
//! to measure the line's own rates for autotune; that needs a managed gateway.

mod measure;
mod run;

use uniwrt_core::UserId;
use uniwrt_model::{
    job::{JobKind, JobNode, JobNodeState, NewJob, StepName},
    node::Node,
};

pub(crate) use run::run;

/// What the transfers fetch: Cloudflare's speed test, 100 MB at a time.
pub(crate) const DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=100000000";
/// Where they post.
pub(crate) const UPLOAD_URL: &str = "https://speed.cloudflare.com/__up";
/// The parallel transfers of a load phase: enough to fill a fast line from one TCP each.
pub(crate) const STREAMS: u32 = 4;
/// The seconds a load phase lasts, warm-up included.
pub(crate) const LOAD_S: u32 = 12;
/// The pings of the idle phase, five a second.
pub(crate) const IDLE_PINGS: u32 = 10;

/// A speed test to run.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpeedTestRequest {
    /// Stop SQM while loading: the line's own rates, for autotune.
    pub bypass_sqm: bool,
}

/// The key that keeps one speed test at a time.
pub(crate) const TARGET: &str = "speed_test";

pub(crate) fn job(gateway: &Node, by: Option<UserId>) -> NewJob {
    NewJob {
        kind: JobKind::SpeedTest,
        target: gateway.name.clone(),
        node: Some(gateway.id),
        revision: None,
        created_by: by,
        steps: vec![
            StepName::Connect,
            StepName::Idle,
            StepName::Download,
            StepName::Upload,
            StepName::Record,
        ],
        nodes: vec![JobNode {
            node: gateway.id,
            name: gateway.name.clone(),
            state: JobNodeState::Pending,
            detail: None,
        }],
    }
}

#[cfg(test)]
mod tests;

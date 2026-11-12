//! Fresh facts after a wpad swap (D-089): the node's capabilities change, which the engine
//! collects hourly, so the job's end has it collect every set again at once.

use std::sync::Arc;

use tokio::sync::broadcast::{Receiver, error::RecvError};
use uniwrt_engine::Engine;
use uniwrt_model::job::{Job, JobKind, JobState};

use crate::fleet_link::FleetLink;

/// Runs until the job runner stops.
pub(crate) async fn after_swaps(engine: Engine<FleetLink>, mut jobs: Receiver<Arc<Job>>) {
    loop {
        match jobs.recv().await {
            Ok(job) if job.kind == JobKind::Wpad && job.state != JobState::Running => {
                if let Some(node) = job.node {
                    engine.recollect(node);
                }
            }
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
    }
}

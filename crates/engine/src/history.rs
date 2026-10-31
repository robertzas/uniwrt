//! The topology's history, for the replay (features/observability.md, Topology): each build as
//! [`Topology::for_history`] keeps it, recorded through [`TopologyRepo`] as a keyframe every
//! hour and as deltas between, at most every 30 s, and pruned to the last 24 h.

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        oneshot,
    },
    task::JoinHandle,
    time::Instant,
};
use tracing::warn;
use uniwrt_core::Timestamp;
use uniwrt_node::NodeLink;
use uniwrt_store::{TopologyFrame, TopologyRepo};
use uniwrt_topo::{Topology, TopologyDelta};

use crate::{Engine, Event, engine::Context};

/// How the history is kept.
#[derive(Debug, Clone, Copy)]
pub struct HistoryConfig {
    /// The least time between two frames: the changes of that time share one.
    pub min_interval: Duration,
    /// How often a whole topology is kept rather than a delta, which bounds how many deltas
    /// a replay applies.
    pub keyframe_every: Duration,
    /// How far back the history goes.
    pub keep: Duration,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_secs(30),
            keyframe_every: Duration::from_hours(1),
            keep: Duration::from_hours(24),
        }
    }
}

/// Records the engine's topology until stopped.
#[derive(Debug)]
pub struct TopologyHistory {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl TopologyHistory {
    /// Starts recording, with the engine's current topology first (once it was built).
    pub fn spawn<L: NodeLink, S: TopologyRepo + 'static>(
        engine: &Engine<L>,
        repo: S,
        config: HistoryConfig,
    ) -> Self {
        let ctx = Arc::clone(engine.context());
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run(ctx, bus, Writer::new(repo, config), stopped));
        Self { stop, task }
    }

    /// Records the last change, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

/// The topology that `frames` make (a keyframe, then deltas, as
/// [`TopologyRepo::topology_frames`] returns them); `None` without a keyframe.
pub fn replay(frames: &[TopologyFrame]) -> Result<Option<Topology>, serde_json::Error> {
    let Some((first, deltas)) = frames.split_first() else {
        return Ok(None);
    };
    if !first.keyframe {
        return Ok(None);
    }
    let mut topology: Topology = serde_json::from_str(&first.data)?;
    for frame in deltas {
        if frame.keyframe {
            topology = serde_json::from_str(&frame.data)?;
        } else {
            topology.apply(&serde_json::from_str::<TopologyDelta>(&frame.data)?);
        }
    }
    Ok(Some(topology))
}

async fn run<S: TopologyRepo>(
    ctx: Arc<Context>,
    mut bus: broadcast::Receiver<Event>,
    mut writer: Writer<S>,
    mut stopped: oneshot::Receiver<()>,
) {
    // Nothing was built yet before the first build.
    let mut pending =
        Some(ctx.topology.current()).filter(|topology| topology.at != Timestamp::UNIX_EPOCH);
    let mut written: Option<Instant> = None;
    loop {
        let due = written.map_or_else(Instant::now, |at| at + writer.config.min_interval);
        if pending.is_some() && due <= Instant::now() {
            if let Some(topology) = pending.take()
                && writer.write(&topology).await
            {
                written = Some(Instant::now());
            }
            continue;
        }
        tokio::select! {
            event = bus.recv() => match event {
                Ok(Event::Topology { topology }) => pending = Some(topology),
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => pending = Some(ctx.topology.current()),
                Err(RecvError::Closed) => return,
            },
            () = tokio::time::sleep_until(due), if pending.is_some() => {}
            _ = &mut stopped => {
                // What the bus holds yet is the latest.
                loop {
                    match bus.try_recv() {
                        Ok(Event::Topology { topology }) => pending = Some(topology),
                        Ok(_) => {}
                        Err(broadcast::error::TryRecvError::Lagged(_)) => {
                            pending = Some(ctx.topology.current());
                        }
                        Err(_) => break,
                    }
                }
                if let Some(topology) = pending {
                    writer.write(&topology).await;
                }
                return;
            }
        }
    }
}

struct Writer<S> {
    repo: S,
    config: HistoryConfig,
    frames: Frames,
}

impl<S: TopologyRepo> Writer<S> {
    fn new(repo: S, config: HistoryConfig) -> Self {
        Self {
            repo,
            config,
            frames: Frames::new(config.keyframe_every),
        }
    }

    /// Records what changed in `topology`; true when a frame was written.
    async fn write(&mut self, topology: &Topology) -> bool {
        let (frame, kept) = match self.frames.next(topology) {
            Ok(Some(next)) => next,
            Ok(None) => return false,
            Err(err) => {
                warn!(%err, "cannot encode the topology");
                return false;
            }
        };
        let (at, keyframe) = (frame.at, frame.keyframe);
        if let Err(err) = self.repo.record_topology(frame).await {
            warn!(%err, "cannot record the topology");
            return false;
        }
        self.frames.recorded(kept);
        if keyframe
            && let Err(err) = self
                .repo
                .prune_topology(at.saturating_sub(self.config.keep))
                .await
        {
            warn!(%err, "cannot prune the topology's history");
        }
        true
    }
}

/// Which frame records a build: the policy, apart from the IO.
struct Frames {
    keyframe_every: Duration,
    /// What was recorded last, and when its keyframe was.
    last: Option<(Topology, Timestamp)>,
}

impl Frames {
    fn new(keyframe_every: Duration) -> Self {
        Self {
            keyframe_every,
            last: None,
        }
    }

    /// The frame that records `topology`, with what it keeps, or `None` when the history
    /// would look the same. [`recorded`](Self::recorded) takes what it keeps once written.
    fn next(
        &self,
        topology: &Topology,
    ) -> serde_json::Result<Option<(TopologyFrame, (Topology, Timestamp))>> {
        let kept = topology.for_history();
        let at = kept.at;
        let keyframe = self.is_keyframe_at(at);
        let (data, keyframe_at) = match &self.last {
            Some((last, keyframe_at)) if !keyframe => {
                let delta = last.diff(&kept);
                if delta.is_empty() {
                    return Ok(None);
                }
                (serde_json::to_string(&delta)?, *keyframe_at)
            }
            _ => (serde_json::to_string(&kept)?, at),
        };
        let frame = TopologyFrame { at, keyframe, data };
        Ok(Some((frame, (kept, keyframe_at))))
    }

    /// A build at `at` is recorded whole: the first, an hour after the last keyframe, or
    /// before it (the clock went back).
    fn is_keyframe_at(&self, at: Timestamp) -> bool {
        self.last.as_ref().is_none_or(|(_, keyframe_at)| {
            at < *keyframe_at || at.saturating_duration_since(*keyframe_at) >= self.keyframe_every
        })
    }

    fn recorded(&mut self, kept: (Topology, Timestamp)) {
        self.last = Some(kept);
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

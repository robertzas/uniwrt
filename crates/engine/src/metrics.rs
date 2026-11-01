//! Recording metrics (`uniwrt_metrics`; features/observability.md, Metrics): a sampler fed by
//! the bus (facts, topology builds, clients' samples) whose closed windows are written every
//! [`MetricsConfig::flush_every`], and the store's upkeep beside it: rollups every
//! [`MetricsConfig::roll_up_every`], retention every [`MetricsConfig::prune_every`].

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        oneshot,
    },
    task::JoinHandle,
};
use tracing::warn;
use uniwrt_metrics::{Retention, Sampler};
use uniwrt_node::{NodeLink, proto::ops::Set};
use uniwrt_store::{MetricsPrune, MetricsRepo};

use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone, Copy)]
pub struct MetricsConfig {
    pub retention: Retention,
    pub flush_every: Duration,
    pub roll_up_every: Duration,
    pub prune_every: Duration,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            retention: Retention::default(),
            flush_every: uniwrt_metrics::WINDOW,
            roll_up_every: Duration::from_mins(1),
            // Often enough that each run deletes little, so the writer is never held long.
            prune_every: Duration::from_mins(15),
        }
    }
}

/// How long after a minute ends its rollup waits for the windows still on their way: a
/// window is written at most a flush and the sampler's grace after it ends.
const ROLL_UP_LAG: Duration = Duration::from_secs(40);
/// The most rollup calls in one round; a longer catch-up goes on next round.
const ROLL_UPS_PER_ROUND: usize = 48;

/// Records the engine's metrics until stopped.
#[derive(Debug)]
pub struct MetricsRecorder {
    stop: oneshot::Sender<()>,
    tasks: [JoinHandle<()>; 2],
}

impl MetricsRecorder {
    pub fn spawn<L: NodeLink, S: MetricsRepo + Clone + 'static>(
        engine: &Engine<L>,
        repo: S,
        config: MetricsConfig,
    ) -> Self {
        let ctx = Arc::clone(engine.context());
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let (stop_upkeep, upkeep_stopped) = oneshot::channel();
        let sampling = tokio::spawn(sample(
            Arc::clone(&ctx),
            bus,
            repo.clone(),
            config,
            stopped,
            stop_upkeep,
        ));
        let upkeep = tokio::spawn(upkeep(ctx, repo, config, upkeep_stopped));
        Self {
            stop,
            tasks: [sampling, upkeep],
        }
    }

    /// Writes every open window, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        for task in self.tasks {
            let _ = task.await;
        }
    }
}

async fn sample<S: MetricsRepo>(
    ctx: Arc<Context>,
    mut bus: broadcast::Receiver<Event>,
    repo: S,
    config: MetricsConfig,
    mut stopped: oneshot::Receiver<()>,
    stop_upkeep: oneshot::Sender<()>,
) {
    let mut sampler = Sampler::new();
    let mut flush = tokio::time::interval(config.flush_every);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = bus.recv() => match event {
                Ok(event) => feed(&mut sampler, &event),
                // Windows that miss some readings average the rest.
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            _ = flush.tick() => write(&repo, &mut sampler, ctx.clock.now()).await,
            _ = &mut stopped => {
                while let Ok(event) = bus.try_recv() {
                    feed(&mut sampler, &event);
                }
                break;
            }
        }
    }
    // Every window, the open ones too: a restart within one replaces it with what follows.
    let end = ctx.clock.now().saturating_add(Duration::from_mins(1));
    write(&repo, &mut sampler, end).await;
    let _ = stop_upkeep.send(());
}

fn feed(sampler: &mut Sampler, event: &Event) {
    match event {
        Event::Facts {
            node,
            facts,
            changed,
        } => {
            sampler.facts(*node, facts, changed.contains(&Set::Devices));
        }
        Event::Topology { topology } => sampler.topology(topology),
        Event::Clients { changes } => sampler.clients(&changes.samples),
        Event::Removed { node } => sampler.forget(*node),
        Event::Status { .. }
        | Event::Node { .. }
        | Event::Roam { .. }
        | Event::Log { .. }
        | Event::Logged { .. } => {}
    }
}

async fn write<S: MetricsRepo>(repo: &S, sampler: &mut Sampler, now: uniwrt_core::Timestamp) {
    let closed = sampler.drain(now);
    if !closed.is_empty()
        && let Err(err) = repo.record_metrics(closed).await
    {
        warn!(%err, "cannot record the metrics");
    }
}

async fn upkeep<S: MetricsRepo>(
    ctx: Arc<Context>,
    repo: S,
    config: MetricsConfig,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut roll_up = tokio::time::interval(config.roll_up_every);
    let mut prune = tokio::time::interval(config.prune_every);
    for timer in [&mut roll_up, &mut prune] {
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    }
    let retention = config.retention;
    loop {
        tokio::select! {
            _ = roll_up.tick() => {
                let now = ctx.clock.now();
                let until = now.saturating_sub(ROLL_UP_LAG);
                let earliest = now.saturating_sub(retention.raw);
                for _ in 0..ROLL_UPS_PER_ROUND {
                    match repo.roll_up_metrics(until, earliest).await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(err) => {
                            warn!(%err, "cannot roll the metrics up");
                            break;
                        }
                    }
                }
            }
            _ = prune.tick() => {
                let now = ctx.clock.now();
                let before = MetricsPrune {
                    raw: now.saturating_sub(retention.raw),
                    minute: now.saturating_sub(retention.minute),
                    hour: now.saturating_sub(retention.hour),
                };
                if let Err(err) = repo.prune_metrics(before).await {
                    warn!(%err, "cannot prune the metrics");
                }
            }
            _ = &mut stopped => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use uniwrt_core::{Clock as _, ManualClock, NodeId, Timestamp};
    use uniwrt_model::{
        facts::NodeFacts,
        metric::{Entity, Metric, Resolution},
    };
    use uniwrt_node::proto::ops::Set;
    use uniwrt_store::{MetricsRepo as _, Store};

    use super::{MetricsConfig, MetricsRecorder};
    use crate::{Engine, EngineConfig, Event, testing::SharedLink};

    /// Real time with short intervals: the store's writer is a thread, so tokio's paused clock
    /// would run ahead of it. Windows follow the manual clock.
    #[tokio::test]
    async fn facts_become_windows_then_rollups() {
        let dir = tempfile::tempdir().unwrap();
        let start = Timestamp::from_unix_secs(1_800_000_000);
        let clock = ManualClock::new(start);
        let store = Store::open(dir.path().join("test.db"), Arc::new(clock.clone()))
            .await
            .unwrap();
        let engine: Engine<SharedLink> =
            Engine::new(EngineConfig::default(), Arc::new(clock.clone()));
        let config = MetricsConfig {
            flush_every: Duration::from_millis(10),
            roll_up_every: Duration::from_millis(10),
            ..MetricsConfig::default()
        };
        let recorder = MetricsRecorder::spawn(&engine, store.clone(), config);
        let mut facts: NodeFacts =
            serde_json::from_str(include_str!("../../model/fixtures/facts/gateway.json")).unwrap();
        let node = NodeId::new(1);
        let entity = Entity::Node(node);
        // Sampling and upkeep are separate tasks on real time, while the test moves the clock
        // in big steps: before the clock moves on, wait for the windows that closed to be
        // stored, so a rollup (40 s behind the clock) never runs ahead of their flush.
        let stored = |windows: usize| {
            let (store, clock) = (store.clone(), clock.clone());
            let entity = entity.clone();
            async move {
                let wait = async {
                    while store
                        .metric_points(&entity, Metric::Cpu, Resolution::Raw, start, clock.now())
                        .await
                        .unwrap()
                        .len()
                        < windows
                    {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                };
                tokio::time::timeout(Duration::from_secs(10), wait)
                    .await
                    .unwrap();
            }
        };
        // Facts every 5 s for three minutes, CPU climbing 1 point each time.
        for i in 0..36_u8 {
            facts.collected_at = clock.now();
            facts.system.cpu_percent = Some(f32::from(i));
            engine.context().publish(Event::Facts {
                node,
                facts: Arc::new(facts.clone()),
                changed: vec![Set::System, Set::Devices],
            });
            tokio::time::sleep(Duration::from_millis(1)).await;
            clock.advance(Duration::from_secs(5));
            // Window j (15 s) closes 5 s after it ends.
            let elapsed = 5 * (u64::from(i) + 1);
            stored(usize::try_from(elapsed.saturating_sub(5) / 15).unwrap()).await;
        }
        // The last windows close; the last minute may not be rolled up yet (40 s behind).
        clock.advance(Duration::from_secs(10));
        stored(12).await;
        // Long enough for the three minutes to be rolled up.
        clock.advance(Duration::from_secs(60));
        let minutes = async {
            loop {
                let minutes = store
                    .metric_points(&entity, Metric::Cpu, Resolution::Minute, start, clock.now())
                    .await
                    .unwrap();
                if minutes.len() == 3 {
                    return minutes;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        let minutes = tokio::time::timeout(Duration::from_secs(10), minutes)
            .await
            .unwrap();
        // Three minutes rolled up, each with its envelope; the raw windows stay.
        assert_eq!(minutes[0].avg, 5.5);
        assert_eq!(minutes[1].avg, 17.5);
        assert_eq!(minutes[2].avg, 29.5);
        assert_eq!((minutes[1].min, minutes[1].max), (13.0, 22.0));
        let end = clock.now();
        let raw = store
            .metric_points(&entity, Metric::Cpu, Resolution::Raw, start, end)
            .await
            .unwrap();
        assert_eq!(raw.len(), 12);
        assert_eq!(raw[0].avg, 1.0);
        // Device rates were recorded too.
        let lan = Entity::Device {
            node,
            name: "br-lan".to_owned(),
        };
        let series = store.metric_series(Some(entity.clone())).await.unwrap();
        assert!(
            series
                .iter()
                .any(|(entity, metrics)| *entity == lan && metrics.contains(&Metric::RxBps))
        );
        recorder.stop().await;
    }
}

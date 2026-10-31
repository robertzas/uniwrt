//! Persisting each node's latest facts ([`FactsRepo`]), so a restarted controller shows them
//! until fresh ones arrive.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::oneshot, task::JoinHandle, time::MissedTickBehavior};
use tracing::warn;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_node::NodeLink;
use uniwrt_store::FactsRepo;

use crate::{Engine, engine::Context};

/// Saves the facts that changed every period, and once more when stopped.
#[derive(Debug)]
pub struct Snapshots {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Snapshots {
    /// Starts saving every `every`, the first time one period from now. Facts the engine
    /// already holds (loaded from `repo`) count as saved.
    pub fn spawn<L: NodeLink, S: FactsRepo + 'static>(
        engine: &Engine<L>,
        repo: S,
        every: Duration,
    ) -> Self {
        let ctx = Arc::clone(engine.context());
        let saved = ctx
            .live
            .all()
            .into_iter()
            .filter_map(|(node, view)| Some((node, view.facts?.collected_at)))
            .collect();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run(ctx, Writer { repo, saved }, every, stopped));
        Self { stop, task }
    }

    /// Saves what changed since the last time, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

async fn run<S: FactsRepo>(
    ctx: Arc<Context>,
    mut writer: Writer<S>,
    every: Duration,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticks.tick() => writer.save(&ctx).await,
            _ = &mut stopped => {
                writer.save(&ctx).await;
                return;
            }
        }
    }
}

struct Writer<S> {
    repo: S,
    /// When the facts saved last of each node were collected.
    saved: HashMap<NodeId, Timestamp>,
}

impl<S: FactsRepo> Writer<S> {
    async fn save(&mut self, ctx: &Context) {
        for (node, view) in ctx.live.all() {
            let Some(facts) = view.facts else {
                continue;
            };
            if self
                .saved
                .get(&node)
                .is_some_and(|&saved| saved >= facts.collected_at)
            {
                continue;
            }
            match self.repo.save_facts(node, &facts).await {
                Ok(()) => {
                    self.saved.insert(node, facts.collected_at);
                }
                Err(err) => warn!(node_id = %node, %err, "cannot save the node's facts"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use serde_json::json;
    use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
    use uniwrt_model::{
        facts::NodeFacts,
        node::{NewNode, NodeRole},
    };
    use uniwrt_node::proto::ops::Set;
    use uniwrt_store::{FactsRepo, NodeRepo, Store};

    use super::Snapshots;
    use crate::{
        Engine, EngineConfig, Event,
        testing::{self, collected, hello, running},
    };

    async fn facts_event(bus: &mut tokio::sync::broadcast::Receiver<Event>) -> Arc<NodeFacts> {
        loop {
            if let Event::Facts { facts, .. } = bus.recv().await.unwrap() {
                return facts;
            }
        }
    }

    async fn saved(store: &Store, node: NodeId, at: Timestamp) -> NodeFacts {
        for _ in 0..500 {
            if let Some(facts) = store.latest_facts(node).await.unwrap()
                && facts.collected_at == at
            {
                return facts;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no snapshot collected at {at:?}");
    }

    #[tokio::test]
    async fn changed_facts_are_saved_each_period_and_once_more_when_stopping() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(Timestamp::from_unix_secs(1_793_200_000)));
        let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
            .await
            .unwrap();
        let node = store
            .create_node(NewNode {
                name: "ap1".to_owned(),
                mac: MacAddr::new([2, 0, 0, 0, 0, 1]),
                board_name: "hwsim".to_owned(),
                host: "10.0.0.2".to_owned(),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let mut bus = engine.subscribe();
        let (link, script) = testing::link();
        engine.add(node.id, (&node).into(), link, None).await;
        let snapshots = Snapshots::spawn(&engine, store.clone(), Duration::from_millis(20));
        script.set_state(running(hello(true)));
        let first = facts_event(&mut bus).await;
        assert_eq!(saved(&store, node.id, first.collected_at).await, *first);

        // Changed facts wait for the next period; stopping saves them at once.
        let snapshots = {
            snapshots.stop().await;
            Snapshots::spawn(&engine, store.clone(), Duration::from_secs(3600))
        };
        clock.advance(Duration::from_secs(15));
        let mut tick = collected(&[Set::System]);
        tick["sets"]["system"]["info"]["uptime"] = json!(1);
        script.push_tick(&tick);
        let second = facts_event(&mut bus).await;
        engine.shutdown().await;
        snapshots.stop().await;
        let stored = store.latest_facts(node.id).await.unwrap().unwrap();
        assert_eq!(
            (stored.collected_at, stored.system),
            (second.collected_at, second.system.clone())
        );
        store.close().await;
    }
}

//! Keeping the clients' book (`uniwrt_clients`; features/observability.md, Clients): restored
//! from the store before the nodes are watched, its records and sessions saved through
//! [`ClientRepo`] as [`Event::Clients`] announces them, and clients long gone (and sessions
//! older than [`RecorderConfig::keep`]) pruned every [`RecorderConfig::prune_every`].

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        oneshot,
    },
    task::JoinHandle,
};
use tracing::warn;
use uniwrt_clients::Changes;
use uniwrt_node::NodeLink;
use uniwrt_store::ClientRepo;

use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone, Copy)]
pub struct RecorderConfig {
    /// How long sessions are kept; the book forgets clients without an alias after its own
    /// `forget_after`, which should match.
    pub keep: Duration,
    pub prune_every: Duration,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            keep: Duration::from_hours(30 * 24),
            prune_every: Duration::from_hours(1),
        }
    }
}

/// Saves the clients' book until stopped.
#[derive(Debug)]
pub struct ClientRecorder {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl ClientRecorder {
    /// Restores the book from `repo` (a store that cannot be read leaves it empty, with a
    /// warning), then records. Start it before adding nodes, so the first builds find their
    /// clients' history.
    pub async fn start<L: NodeLink, S: ClientRepo + 'static>(
        engine: &Engine<L>,
        repo: S,
        config: RecorderConfig,
    ) -> Self {
        let ctx = Arc::clone(engine.context());
        let bus = engine.subscribe();
        let restored = async {
            Ok::<_, uniwrt_store::Error>((repo.clients().await?, repo.latest_sessions().await?))
        };
        match restored.await {
            Ok((records, sessions)) => {
                let changes = ctx.clients().restore(records, sessions);
                ctx.publish_clients(changes);
            }
            Err(err) => warn!(%err, "cannot read the clients; starting without their history"),
        }
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run(ctx, bus, repo, config, stopped));
        Self { stop, task }
    }

    /// Saves what is left, and stops.
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

async fn run<S: ClientRepo>(
    ctx: Arc<Context>,
    mut bus: broadcast::Receiver<Event>,
    repo: S,
    config: RecorderConfig,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut prune = tokio::time::interval(config.prune_every);
    loop {
        tokio::select! {
            event = bus.recv() => match event {
                Ok(Event::Clients { changes }) => save(&repo, &changes).await,
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    let unsaved = ctx.clients().unsaved();
                    save(&repo, &unsaved).await;
                }
                Err(RecvError::Closed) => return,
            },
            _ = prune.tick() => {
                let now = ctx.clock.now();
                let changes = ctx.clients().prune(now);
                ctx.publish_clients(changes);
                if let Err(err) = repo.prune_clients(now.saturating_sub(config.keep)).await {
                    warn!(%err, "cannot prune the clients");
                }
            }
            _ = &mut stopped => {
                // What the bus holds yet, then whatever was not saved.
                while let Ok(event) = bus.try_recv() {
                    if let Event::Clients { changes } = event {
                        save(&repo, &changes).await;
                    }
                }
                let unsaved = ctx.clients().unsaved();
                save(&repo, &unsaved).await;
                return;
            }
        }
    }
}

async fn save<S: ClientRepo>(repo: &S, changes: &Changes) {
    if !changes.records.is_empty()
        && let Err(err) = repo.save_clients(changes.records.clone()).await
    {
        warn!(%err, "cannot save the clients");
    }
    if !changes.sessions.is_empty()
        && let Err(err) = repo.save_sessions(changes.sessions.clone()).await
    {
        warn!(%err, "cannot save the clients' sessions");
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use uniwrt_core::{Clock as _, ManualClock, NodeId, Timestamp};
    use uniwrt_model::client::{ClientMedium, ClientRecord, ClientSession};
    use uniwrt_store::{ClientRepo as _, Store};

    use super::{ClientRecorder, RecorderConfig};
    use crate::{Engine, EngineConfig, testing::SharedLink};

    #[tokio::test(start_paused = true)]
    async fn the_book_is_restored_saved_and_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(Timestamp::from_unix_secs(1_800_000_000));
        let store = Store::open(dir.path().join("test.db"), Arc::new(clock.clone()))
            .await
            .unwrap();
        let mac = "f0:18:98:00:00:01".parse().unwrap();
        let old = "f0:18:98:00:00:02".parse().unwrap();
        let record = |mac, last_seen| ClientRecord {
            mac,
            alias: None,
            hostname: None,
            first_seen: Timestamp::from_unix_secs(1_000),
            last_seen,
            blocked: None,
        };
        store
            .save_clients(vec![
                record(mac, clock.now()),
                record(old, Timestamp::from_unix_secs(1_000)),
            ])
            .await
            .unwrap();
        let open = ClientSession {
            mac,
            node: Some(NodeId::new(1)),
            port: Some("phy1-ap0".to_owned()),
            medium: ClientMedium::Wifi,
            band: None,
            ssid: None,
            start: Timestamp::from_unix_secs(1_500),
            end: None,
            ended_by: None,
        };
        store.save_sessions(vec![open.clone()]).await.unwrap();

        let engine: Engine<SharedLink> =
            Engine::new(EngineConfig::default(), Arc::new(clock.clone()));
        let recorder =
            ClientRecorder::start(&engine, store.clone(), RecorderConfig::default()).await;
        // The first prune forgets the client gone for decades.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let clients = engine.clients();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].connection.as_ref().unwrap().since, open.start);

        assert!(
            engine
                .rename_client(mac, Some("Phone".to_owned()))
                .is_some()
        );
        recorder.stop().await;
        // Stopping saved the open session again; nothing ended it.
        assert_eq!(store.latest_sessions().await.unwrap(), [open]);
        assert_eq!(store.clients().await.unwrap().len(), 1);
    }
}

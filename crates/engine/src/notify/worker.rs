//! One notifier's deliveries, in order: what arrives within [`NotifyConfig::gather`] of the
//! first goes out together (one alert as itself, several as a digest of at most
//! [`MAX_DIGEST`]), and a delivery that fails in a way that may pass is tried again after each
//! of [`NotifyConfig::retries`]. Each outcome is recorded on the notifier.

use std::sync::Arc;

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};
use uniwrt_core::{Clock, NotifierId};
use uniwrt_model::notify::NotifierSpec;
use uniwrt_notify::{Item, Message, Secrets, SendError, Sender};
use uniwrt_store::NotifierRepo;

use super::NotifyConfig;

/// The most alerts one message tells of.
pub(super) const MAX_DIGEST: usize = 25;

/// A running worker; dropping it stops it.
pub(super) struct Worker {
    queue: mpsc::Sender<Item>,
    task: JoinHandle<()>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// What every worker shares.
pub(super) struct Shared<R> {
    pub(super) repo: R,
    pub(super) sender: Sender,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) config: NotifyConfig,
}

impl Worker {
    pub(super) fn start<R: NotifierRepo + 'static>(
        id: NotifierId,
        spec: NotifierSpec,
        secrets: Secrets,
        link_url: Option<String>,
        shared: Arc<Shared<R>>,
    ) -> Self {
        let (queue, received) = mpsc::channel(shared.config.queue);
        let task = tokio::spawn(run(id, spec, secrets, link_url, shared, received));
        Self { queue, task }
    }

    /// Queues an alert; a full queue drops it.
    pub(super) fn push(&self, id: NotifierId, item: Item) {
        if self.queue.try_send(item).is_err() {
            warn!(%id, "the notifier's queue is full: a notification is dropped");
        }
    }
}

async fn run<R: NotifierRepo>(
    id: NotifierId,
    spec: NotifierSpec,
    secrets: Secrets,
    link_url: Option<String>,
    shared: Arc<Shared<R>>,
    mut received: mpsc::Receiver<Item>,
) {
    while let Some(first) = received.recv().await {
        let mut items = vec![first];
        let gathered = tokio::time::sleep(shared.config.gather);
        tokio::pin!(gathered);
        while items.len() < MAX_DIGEST {
            tokio::select! {
                Some(item) = received.recv() => items.push(item),
                () = &mut gathered => break,
            }
        }
        let message = Message::new(items, link_url.as_deref(), shared.clock.now());
        let outcome = deliver(&spec, &secrets, &message, &shared).await;
        let error = outcome.err().map(|err| err.to_string());
        if let Err(err) = shared
            .repo
            .record_delivery(id, shared.clock.now(), error)
            .await
        {
            warn!(%id, %err, "cannot record how a notification went");
        }
    }
}

async fn deliver<R>(
    spec: &NotifierSpec,
    secrets: &Secrets,
    message: &Message,
    shared: &Shared<R>,
) -> Result<(), SendError> {
    let mut retries = shared.config.retries.iter();
    loop {
        match shared.sender.send(spec, secrets, message).await {
            Ok(()) => {
                debug!(kind = spec.kind(), title = %message.title, "notified");
                return Ok(());
            }
            Err(err) => match retries.next() {
                Some(wait) if err.retryable() => {
                    warn!(kind = spec.kind(), %err, "a notification failed: trying again");
                    tokio::time::sleep(*wait).await;
                }
                _ => {
                    warn!(kind = spec.kind(), %err, "a notification failed");
                    return Err(err);
                }
            },
        }
    }
}

//! Notifications (features/observability.md, Notifications; `uniwrt_notify`): the alerts the
//! alert engine stores ([`Event::Alerts`]) go through the notifiers' [`Policy`], which is also
//! ticked for reminders; what it decides goes to each notifier's worker, which gathers what
//! arrives together into one message, delivers it with its secrets from the vault, tries again
//! when that may help, and records how it went.
//!
//! Start it before the alert engine, so that it hears of what that one stores at its start; it
//! resumes the alerts already firing as heard of. The API changes notifiers and their settings
//! in the store and the vault, then [`Notify::reload`]s; [`Notify::test`] sends a test.

mod worker;

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    task::JoinHandle,
};
use tracing::warn;
use uniwrt_core::{Clock, NotifierId};
use uniwrt_model::{
    alerts::AlertFilter,
    notify::{NotificationSettings, NotifierConfig, NotifierSpec},
};
use uniwrt_node::NodeLink;
use uniwrt_notify::{Item, Message, Names, Policy, Secrets, Send, SendError, Sender};
use uniwrt_store::{AlertRepo, NodeRepo, NotifierRepo, SettingsRepo, VaultRepo};

use self::worker::{Shared, Worker};
use crate::{Engine, Event, engine::Context};

#[derive(Debug, Clone)]
pub struct NotifyConfig {
    /// How often reminders are looked at.
    pub tick_every: Duration,
    /// How long a worker waits after an alert for more, to send them together.
    pub gather: Duration,
    /// How long to wait before each new try of a delivery that failed.
    pub retries: Vec<Duration>,
    /// How many alerts may wait for one notifier.
    pub queue: usize,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            gather: Duration::from_secs(2),
            retries: vec![
                Duration::from_secs(15),
                Duration::from_mins(1),
                Duration::from_mins(5),
            ],
            queue: 256,
        }
    }
}

/// The notifications stopped.
#[derive(Debug, thiserror::Error)]
#[error("notifications are not running")]
pub struct NotifyStopped;

/// The API's way to the notifications. Clones share it.
#[derive(Clone)]
pub struct Notify {
    commands: mpsc::Sender<Command>,
    sender: Sender,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Notify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notify").finish_non_exhaustive()
    }
}

enum Command {
    Reload(oneshot::Sender<()>),
}

impl Notify {
    /// Reads the notifiers, their secrets and the settings again, once they changed; returns
    /// once they apply.
    pub async fn reload(&self) -> Result<(), NotifyStopped> {
        let (done, applied) = oneshot::channel();
        self.commands
            .send(Command::Reload(done))
            .await
            .map_err(|_| NotifyStopped)?;
        applied.await.map_err(|_| NotifyStopped)
    }

    /// Sends a test through a notifier, stored or not, at once and once.
    pub async fn test(
        &self,
        spec: &NotifierSpec,
        secrets: &Secrets,
        link_url: Option<&str>,
    ) -> Result<(), SendError> {
        let message = Message::test(link_url, self.clock.now());
        self.sender.send(spec, secrets, &message).await
    }
}

/// Sends notifications until stopped.
pub struct Notifications {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
    handle: Notify,
}

impl Notifications {
    /// Starts following what the alert engine stores.
    pub async fn start<L, R, V>(
        engine: &Engine<L>,
        repo: R,
        vault: V,
        sender: Sender,
        config: NotifyConfig,
    ) -> Self
    where
        L: NodeLink,
        R: NotifierRepo + NodeRepo + SettingsRepo + AlertRepo + Clone + 'static,
        V: VaultRepo + 'static,
    {
        let bus = engine.subscribe();
        let ctx = Arc::clone(engine.context());
        let clock = Arc::clone(&ctx.clock);
        let tick_every = config.tick_every;
        let mut task = Task {
            ctx,
            vault,
            shared: Arc::new(Shared {
                repo: repo.clone(),
                sender: sender.clone(),
                clock: Arc::clone(&clock),
                config,
            }),
            repo,
            policy: Policy::new(),
            workers: HashMap::new(),
            link_url: None,
        };
        task.reload().await;
        let active = AlertFilter {
            active: true,
            ..AlertFilter::default()
        };
        match task.repo.alerts(active, None, 10_000).await {
            Ok(firing) => task.policy.resume(firing, clock.now()),
            Err(err) => warn!(%err, "cannot read the alerts firing: they may be notified again"),
        }
        let (commands, received) = mpsc::channel(8);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(task.run(bus, received, tick_every, stopped));
        Self {
            stop,
            task,
            handle: Notify {
                commands,
                sender,
                clock,
            },
        }
    }

    pub fn handle(&self) -> Notify {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

/// What a worker was started with: one that changed is started again.
#[derive(PartialEq, Eq)]
struct Started {
    config: NotifierConfig,
    secrets: Secrets,
    link_url: Option<String>,
}

struct Task<R, V> {
    ctx: Arc<Context>,
    repo: R,
    vault: V,
    shared: Arc<Shared<R>>,
    policy: Policy,
    workers: HashMap<NotifierId, (Started, Worker)>,
    link_url: Option<String>,
}

impl<R, V> Task<R, V>
where
    R: NotifierRepo + NodeRepo + SettingsRepo + Clone + 'static,
    V: VaultRepo,
{
    async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut commands: mpsc::Receiver<Command>,
        tick_every: Duration,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let sends = tokio::select! {
                event = bus.recv() => match event {
                    Ok(Event::Alerts { alerts }) => {
                        let now = self.ctx.clock.now();
                        self.policy.alerts(&alerts, now)
                    }
                    Ok(_) => Vec::new(),
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "notifications fell behind the bus: alerts may be missed");
                        Vec::new()
                    }
                    Err(RecvError::Closed) => break,
                },
                Some(command) = commands.recv() => match command {
                    Command::Reload(done) => {
                        self.reload().await;
                        let _ = done.send(());
                        Vec::new()
                    }
                },
                _ = tick.tick() => {
                    let now = self.ctx.clock.now();
                    self.policy.tick(now)
                }
                _ = &mut stopped => break,
            };
            self.dispatch(sends).await;
        }
    }

    /// The enabled notifiers, their secrets and the settings, from the store and the vault.
    async fn reload(&mut self) {
        let link_url = match self.repo.setting(NotificationSettings::KEY).await {
            Ok(Some(value)) => {
                serde_json::from_value::<NotificationSettings>(value)
                    .unwrap_or_default()
                    .link_url
            }
            Ok(None) => None,
            Err(err) => {
                warn!(%err, "cannot read the notification settings");
                self.link_url.clone()
            }
        };
        let notifiers = match self.repo.notifiers().await {
            Ok(notifiers) => notifiers,
            Err(err) => {
                warn!(%err, "cannot read the notifiers");
                return;
            }
        };
        let mut workers = HashMap::new();
        let mut filters = Vec::new();
        for notifier in notifiers.into_iter().filter(|n| n.config.enabled) {
            let id = notifier.id;
            let started = Started {
                secrets: self.secrets(id).await,
                config: notifier.config,
                link_url: link_url.clone(),
            };
            filters.push((id, started.config.filter.clone()));
            let worker = match self.workers.remove(&id) {
                Some((before, worker)) if before == started => worker,
                _ => Worker::start(
                    id,
                    started.config.spec.clone(),
                    started.secrets.clone(),
                    link_url.clone(),
                    Arc::clone(&self.shared),
                ),
            };
            workers.insert(id, (started, worker));
        }
        // The workers of notifiers gone or changed stop as they drop.
        self.workers = workers;
        self.policy.set_notifiers(filters);
        self.link_url = link_url;
    }

    async fn secrets(&self, id: NotifierId) -> Secrets {
        match self.vault.secret(&Secrets::vault_name(id)).await {
            Ok(Some(sealed)) => Secrets::from_json(&sealed).unwrap_or_else(|err| {
                warn!(%id, %err, "the notifier's secrets are unreadable");
                Secrets::default()
            }),
            Ok(None) => Secrets::default(),
            Err(err) => {
                warn!(%id, %err, "cannot read the notifier's secrets");
                Secrets::default()
            }
        }
    }

    /// Hands each send to its notifier's worker, in words.
    async fn dispatch(&self, sends: Vec<Send>) {
        if sends.is_empty() {
            return;
        }
        let names = self.names(&sends).await;
        for send in sends {
            if let Some((_, worker)) = self.workers.get(&send.notifier) {
                let item = Item::new(send.kind, send.alert, &names, self.link_url.as_deref());
                worker.push(send.notifier, item);
            }
        }
    }

    /// What the nodes and the clients of `sends` are called.
    async fn names(&self, sends: &[Send]) -> Names {
        let mut names = Names::default();
        match self.repo.nodes().await {
            Ok(nodes) => names.nodes = nodes.into_iter().map(|n| (n.id, n.name)).collect(),
            Err(err) => warn!(%err, "cannot read the nodes' names"),
        }
        let macs: BTreeSet<_> = sends.iter().filter_map(|s| s.alert.state.client).collect();
        if !macs.is_empty() {
            let clients = self.ctx.clients().clients();
            names.clients = clients
                .iter()
                .filter(|c| macs.contains(&c.mac))
                .filter_map(|c| Some((c.mac, c.alias.clone().or_else(|| c.hostname.clone())?)))
                .collect();
        }
        names
    }
}

#[cfg(test)]
mod tests;

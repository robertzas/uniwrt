//! The Tailscale task: when to compare, what to send, and what to raise.

use std::{collections::BTreeMap, sync::Arc};

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot,
};
use tracing::{debug, warn};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    job::{Job, JobState},
    node::NodeRole,
    site::{SiteConfig, vpn::tailscale::Prefs},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{Set, Tailscale, TailscaleAction},
};
use uniwrt_store::{RevisionRepo, VaultRepo};

use super::{TAILSCALE_TIMEOUT, TailscaleConfig, step};
use crate::{Engine, Event, wlans::next_job};

/// What was last sent, and when.
struct Sent {
    at: Timestamp,
    action: TailscaleAction,
}

/// A request that ended.
struct Done {
    node: NodeId,
    result: Result<(), String>,
}

pub(super) struct Task<L, R, V> {
    engine: Engine<L>,
    repo: R,
    vault: Option<V>,
    config: TailscaleConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    sent: Option<Sent>,
    running: bool,
    /// The gateway whose latest request failed (its condition is raised).
    failing: Option<NodeId>,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

impl<L, R, V> Task<L, R, V>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
    V: VaultRepo + Clone + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        vault: Option<V>,
        config: TailscaleConfig,
    ) -> Self {
        Self {
            engine,
            repo,
            vault,
            config,
            head: None,
            jobs: BTreeMap::new(),
            sent: None,
            running: false,
            failing: None,
            done: mpsc::channel(4),
        }
    }

    pub(super) async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut jobs: Option<broadcast::Receiver<Arc<Job>>>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                Some(job) = next_job(&mut jobs) => self.on_job(&job),
                Some(done) = self.done.1.recv() => self.on_done(done),
                event = bus.recv() => match event {
                    Ok(Event::Facts { changed, .. }) if changed.contains(&Set::Services) => {
                        self.compare();
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
                _ = tick.tick() => {
                    self.reload().await;
                    self.compare();
                }
            }
        }
    }

    /// Reads the head revision when it moved; a new one may want another request at once.
    async fn reload(&mut self) {
        match self.repo.head_revision().await {
            Ok(Some(head)) if self.head.as_ref().is_none_or(|(id, _)| *id != head.meta.id) => {
                self.head = Some((head.meta.id, Arc::new(head.config)));
                self.sent = None;
            }
            Ok(_) => {}
            Err(err) => warn!(%err, "Tailscale: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else if self.jobs.remove(&job.id).is_some() {
            // An apply may have installed the package, a reboot restarted the daemon.
            self.sent = None;
            self.compare();
        }
    }

    /// The managed gateway, online, that no job holds.
    fn gateway(&self, config: &SiteConfig) -> Option<NodeId> {
        let topology = &self.engine.context().topology;
        let (node, view) = self
            .engine
            .all()
            .into_iter()
            .find(|&(node, _)| topology.role(node) == Some(NodeRole::Gateway))?;
        let managed = config
            .nodes
            .iter()
            .any(|n| n.node == node && !n.managed.is_empty());
        let busy = self.jobs.values().any(|nodes| nodes.contains(&node));
        (managed && !busy && view.status.state != NodeState::Offline).then_some(node)
    }

    fn compare(&mut self) {
        let Some((_, config)) = self.head.clone() else {
            return;
        };
        let Some(node) = self.gateway(&config) else {
            return;
        };
        let facts = self.engine.node(node).and_then(|view| view.facts);
        let Some(facts) = facts.as_ref().and_then(|f| f.services.tailscale.as_deref()) else {
            return;
        };
        if let Some((action, prefs)) = step(&config, facts) {
            self.act(node, &config, action, prefs);
        }
    }

    fn act(&mut self, node: NodeId, config: &SiteConfig, action: TailscaleAction, prefs: Prefs) {
        if self.running {
            return;
        }
        let now = self.engine.context().clock.now();
        if let Some(sent) = &self.sent {
            let wait = if sent.action == action {
                self.config.retry
            } else {
                self.config.settle
            };
            if now.saturating_duration_since(sent.at) < wait {
                return;
            }
        }
        let Some(link) = self.engine.link(node) else {
            return;
        };
        let key = match action {
            TailscaleAction::Login => {
                let (Some(vault), Some(key)) = (
                    self.vault.clone(),
                    config
                        .vpn
                        .tailscale
                        .as_ref()
                        .and_then(|t| t.auth_key.clone()),
                ) else {
                    return;
                };
                Some((vault, key))
            }
            _ => None,
        };
        debug!(%node, ?action, "Tailscale: bringing the gateway's daemon to the site");
        self.sent = Some(Sent { at: now, action });
        self.running = true;
        let done = self.done.0.clone();
        tokio::spawn(async move {
            let request = match key {
                Some((vault, key)) => match vault.secret(key.name()).await {
                    Ok(Some(secret)) => match std::str::from_utf8(&secret) {
                        Ok(text) => Ok(Tailscale::login_with(prefs, text.to_owned())),
                        Err(_) => Err(format!("{} is not UTF-8", key.name())),
                    },
                    Ok(None) => Err(format!("the vault lacks {}", key.name())),
                    Err(err) => Err(err.to_string()),
                },
                None => Ok(Tailscale::new(action, prefs)),
            };
            let result = match request {
                Ok(request) => link
                    .request(&request, TAILSCALE_TIMEOUT)
                    .await
                    .map(|_| ())
                    .map_err(|err| match err {
                        // What the node said, for people; the code is for the controller.
                        LinkError::Remote { msg, .. } => msg,
                        other => other.to_string(),
                    }),
                Err(why) => Err(why),
            };
            let _ = done.send(Done { node, result }).await;
        });
    }

    fn on_done(&mut self, Done { node, result }: Done) {
        self.running = false;
        let at = self.engine.context().clock.now();
        let condition = format!("node:{node}/tailscale");
        match result {
            Ok(()) => {
                if self.failing.take().is_some() {
                    let event = NewEvent::new(at, EventKind::TailscaleOk).node(node);
                    self.engine.note(event.condition(condition, false));
                }
            }
            Err(error) => {
                warn!(%node, %error, "Tailscale: the gateway refused");
                if self.failing.replace(node).is_none() {
                    let event = NewEvent::new(at, EventKind::TailscaleFailed { error }).node(node);
                    self.engine
                        .note(event.condition(condition.clone(), true).fold(condition));
                }
            }
        }
    }
}

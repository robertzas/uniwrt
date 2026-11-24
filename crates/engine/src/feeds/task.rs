//! The mirror's task: the plan made from the store, the feeds read from nodes that have none
//! kept, and when to sync.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use tokio::sync::{mpsc, oneshot, watch};
use tracing::{info, warn};
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_feeds::{Mirror, Plan};
use uniwrt_model::{
    facts::NodeFacts,
    feeds::MirrorSettings,
    inventory::{Inventory, NodeFeed},
    status::NodeState,
};
use uniwrt_node::{ExecRequest, NodeLink, pkg};
use uniwrt_store::{FactsRepo, InventoryRepo, SettingsRepo};

use super::{
    FeedMirrorConfig, FeedMirrorError,
    next::{self, Lookups, NextSource},
    plan::plan,
};
use crate::Engine;

pub(super) enum Command {
    Refresh {
        reply: oneshot::Sender<Result<(), FeedMirrorError>>,
    },
    Reload {
        reply: oneshot::Sender<()>,
    },
}

enum Done {
    /// A sync ended; `ok` when nothing in it failed.
    Synced { ok: bool },
    /// A node's feeds were read (`None`: the read failed).
    Read {
        node: NodeId,
        feeds: Option<Vec<NodeFeed>>,
    },
    /// A node's upgrades on offer were looked up, while it ran `version`.
    Next {
        node: NodeId,
        version: String,
        answer: Result<Vec<uniwrt_model::feeds::NextRelease>, String>,
    },
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    mirror: Mirror,
    config: FeedMirrorConfig,
    settings: watch::Sender<MirrorSettings>,
    /// `None` until the settings were read.
    read_settings: Option<MirrorSettings>,
    /// `None` until the inventories and facts were read.
    plan: Option<Plan>,
    /// The plan of the last sync started.
    synced: Option<Plan>,
    syncing: bool,
    /// When a refresh last started.
    refreshed: Option<Timestamp>,
    /// When the last sync that failed in part ended; `None` after one that didn't.
    failed: Option<Timestamp>,
    /// A refresh was asked for.
    asked: bool,
    /// When each node's feeds were last read.
    reads: BTreeMap<NodeId, Timestamp>,
    /// The feeds read from watched nodes that have no inventory stored.
    unstored: BTreeMap<NodeId, Vec<NodeFeed>>,
    reading: BTreeSet<NodeId>,
    /// The releases each node is offered.
    lookups: Lookups,
    done: (mpsc::Sender<Done>, mpsc::Receiver<Done>),
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: InventoryRepo + FactsRepo + SettingsRepo + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        mirror: Mirror,
        config: FeedMirrorConfig,
        settings: watch::Sender<MirrorSettings>,
    ) -> Self {
        Self {
            engine,
            repo,
            mirror,
            config,
            settings,
            read_settings: None,
            plan: None,
            synced: None,
            syncing: false,
            refreshed: None,
            failed: None,
            asked: false,
            reads: BTreeMap::new(),
            unstored: BTreeMap::new(),
            reading: BTreeSet::new(),
            lookups: Lookups::default(),
            done: mpsc::channel(8),
        }
    }

    pub(super) async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                Some(command) = commands.recv() => match command {
                    Command::Refresh { reply } => {
                        self.reload().await;
                        let on = self.read_settings.is_some_and(|s| s.enabled);
                        self.asked |= on;
                        self.step();
                        let _ = reply.send(if on { Ok(()) } else { Err(FeedMirrorError::Off) });
                    }
                    Command::Reload { reply } => {
                        self.reload().await;
                        self.step();
                        let _ = reply.send(());
                    }
                },
                Some(done) = self.done.1.recv() => self.on_done(done).await,
                _ = tick.tick() => self.reload().await,
            }
            self.step();
        }
    }

    /// Reads the settings, the inventories and the nodes' facts, and plans from them.
    async fn reload(&mut self) {
        let settings = match self.repo.setting(MirrorSettings::KEY).await {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_else(|err| {
                warn!(%err, "feed mirror: the settings don't read; the defaults apply");
                MirrorSettings::default()
            }),
            Ok(None) => MirrorSettings::default(),
            Err(err) => {
                warn!(%err, "feed mirror: cannot read the settings");
                return;
            }
        };
        let inventories = match self.repo.inventories().await {
            Ok(inventories) => inventories,
            Err(err) => {
                warn!(%err, "feed mirror: cannot read the inventories");
                return;
            }
        };
        let mut nodes: Vec<(NodeId, Arc<NodeFacts>, Inventory)> = Vec::new();
        for (node, inventory) in inventories {
            let live = self.engine.node(node).and_then(|view| view.facts);
            let facts = match live {
                Some(facts) => facts,
                None => match self.repo.latest_facts(node).await {
                    Ok(Some(facts)) => Arc::new(facts),
                    Ok(None) => continue,
                    Err(err) => {
                        warn!(%err, %node, "feed mirror: cannot read the node's facts");
                        return;
                    }
                },
            };
            nodes.push((node, facts, inventory));
        }
        for (node, facts, inventory) in &nodes {
            if inventory.feeds.is_empty() {
                self.read_feeds(*node, facts);
            }
        }
        // Watched nodes without an inventory (none is stored before adoption reads it).
        self.unstored
            .retain(|node, _| nodes.iter().all(|(n, ..)| n != node));
        for (node, view) in self.engine.all() {
            let Some(facts) = view
                .facts
                .filter(|_| nodes.iter().all(|(n, ..)| *n != node))
            else {
                continue;
            };
            match self.unstored.get(&node) {
                Some(feeds) => {
                    let inventory = Inventory {
                        taken_at: facts.collected_at,
                        packages: Vec::new(),
                        feeds: feeds.clone(),
                    };
                    nodes.push((node, facts, inventory));
                }
                None => self.read_feeds(node, &facts),
            }
        }
        if self.look_up_next(&nodes) {
            let lookups = &self.lookups;
            self.plan = Some(plan(
                nodes.iter().map(|(node, facts, inventory)| {
                    let next = next::running(facts).map_or(&[][..], |v| lookups.of(*node, v));
                    (&**facts, inventory, next)
                }),
                settings.whole_feeds,
            ));
        }
        self.read_settings = Some(settings);
        self.config.via.set(settings.enabled);
        self.settings.send_if_modified(|current| {
            let moved = *current != settings;
            *current = settings;
            moved
        });
    }

    /// Looks up the upgrades on offer of the nodes due; whether every node has been looked up
    /// for the release it runs (always, without a source).
    fn look_up_next(&mut self, nodes: &[(NodeId, Arc<NodeFacts>, Inventory)]) -> bool {
        let Some(source) = self.config.next.clone() else {
            return true;
        };
        let now = self.engine.context().clock.now();
        self.lookups
            .retain(|node| nodes.iter().any(|(known, ..)| *known == node));
        for (node, facts, inventory) in nodes {
            let Some(version) = next::running(facts) else {
                continue;
            };
            if self
                .lookups
                .due(*node, version, now, self.config.next_every)
            {
                self.lookups.start(*node, version.to_owned(), now);
                let (source, done) = (source.clone(), self.done.0.clone());
                let (node, facts, inventory) = (*node, facts.clone(), inventory.clone());
                let timeout = self.config.next_timeout;
                tokio::spawn(async move {
                    let NextSource(source) = source;
                    let looked = source.next(node, &facts, &inventory);
                    let answer = tokio::time::timeout(timeout, looked)
                        .await
                        .unwrap_or_else(|_| Err("the lookup timed out".to_owned()));
                    let version = facts.board.release.version.clone();
                    let _ = done
                        .send(Done::Next {
                            node,
                            version,
                            answer,
                        })
                        .await;
                });
            }
        }
        nodes.iter().all(|(node, facts, _)| {
            next::running(facts).is_none_or(|version| self.lookups.settled(*node, version))
        })
    }

    /// Reads `node`'s feeds over its link, unless it is offline, being read, or was read lately.
    fn read_feeds(&mut self, node: NodeId, facts: &NodeFacts) {
        let now = self.engine.context().clock.now();
        let Some(manager) = facts.capabilities.package_manager else {
            return;
        };
        let lately = self
            .reads
            .get(&node)
            .is_some_and(|at| now.saturating_duration_since(*at) < self.config.read_every);
        let online = self
            .engine
            .node(node)
            .is_some_and(|view| view.status.state != NodeState::Offline);
        if lately || !online || self.reading.contains(&node) {
            return;
        }
        let Some(link) = self.engine.link(node) else {
            return;
        };
        self.reads.insert(node, now);
        self.reading.insert(node);
        let (done, timeout) = (self.done.0.clone(), self.config.timeout);
        tokio::spawn(async move {
            let request = ExecRequest::new(pkg::feeds::command(manager));
            let read = tokio::time::timeout(timeout, link.exec(request)).await;
            let feeds = match read {
                Ok(Ok(output)) if output.status.success() => Some(pkg::feeds::parse(
                    manager,
                    &String::from_utf8_lossy(&output.stdout),
                )),
                _ => None,
            };
            let _ = done.send(Done::Read { node, feeds }).await;
        });
    }

    /// Starts the sync that is due, if any.
    fn step(&mut self) {
        let (Some(settings), Some(plan)) = (self.read_settings, self.plan.as_ref()) else {
            return;
        };
        if !settings.enabled || self.syncing {
            return;
        }
        let now = self.engine.context().clock.now();
        let since = |at: Timestamp| now.saturating_duration_since(at);
        let refresh = self.asked
            || self
                .refreshed
                .is_none_or(|at| since(at) >= self.config.refresh_every)
            || self
                .failed
                .is_some_and(|at| since(at) >= self.config.retry_after);
        if !refresh && self.synced.as_ref() == Some(plan) {
            return;
        }
        let plan = plan.clone();
        self.syncing = true;
        self.synced = Some(plan.clone());
        if refresh {
            self.asked = false;
            self.refreshed = Some(now);
        }
        let (mirror, done) = (self.mirror.clone(), self.done.0.clone());
        tokio::spawn(async move {
            let ok = match mirror.sync(plan, refresh).await {
                Ok(()) => match mirror.status().sync_error {
                    Some(err) => {
                        warn!(%err, "feed mirror: the sync failed in part");
                        false
                    }
                    None => true,
                },
                Err(err) => {
                    warn!(%err, "feed mirror: cannot sync");
                    false
                }
            };
            let _ = done.send(Done::Synced { ok }).await;
        });
    }

    async fn on_done(&mut self, done: Done) {
        let now = self.engine.context().clock.now();
        match done {
            Done::Synced { ok } => {
                self.syncing = false;
                self.failed = (!ok).then_some(now);
            }
            Done::Next {
                node,
                version,
                answer,
            } => {
                self.lookups.finish(node, version, answer);
                if self.lookups.idle() {
                    self.reload().await;
                }
            }
            Done::Read { node, feeds } => {
                self.reading.remove(&node);
                let Some(feeds) = feeds.filter(|feeds| !feeds.is_empty()) else {
                    return;
                };
                // Kept on the inventory as it is now: a job may have read it again since.
                match self.repo.inventory(node).await {
                    Ok(Some(mut inventory)) if inventory.feeds.is_empty() => {
                        inventory.feeds = feeds;
                        match self.repo.set_inventory(node, inventory).await {
                            Ok(()) => {
                                info!(%node, "feed mirror: the node's feeds are read");
                                self.reload().await;
                            }
                            Err(err) => warn!(%err, %node, "feed mirror: cannot keep the feeds"),
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        self.unstored.insert(node, feeds);
                        self.reload().await;
                    }
                    Err(err) => warn!(%err, %node, "feed mirror: cannot read the inventory"),
                }
            }
        }
    }
}

//! The WAN monitor's task: a round, its samples, and the outage it may start or end.

use std::{net::IpAddr, sync::Arc};

use tokio::sync::{Notify, oneshot, watch};
use tracing::warn;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    events::{EventKind, NewEvent},
    metric::{Entity, Metric, Sample},
    node::NodeRole,
    status::NodeState,
    wan_monitor::{Probe, ProbeRound, WanFault, WanHealth, WanMonitorSettings, assess},
};
use uniwrt_node::{NodeLink, probe};
use uniwrt_store::{EventRepo, MetricsRepo, SettingsRepo};

use super::{OUTAGE_CONDITION, WanMonitorConfig, WanMonitorState};
use crate::{Engine, Event};

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: WanMonitorConfig,
    state: watch::Sender<Arc<WanMonitorState>>,
    poke: Arc<Notify>,
    /// The outage raised: where it was placed, and since when.
    outage: Option<(WanFault, Timestamp)>,
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: SettingsRepo + MetricsRepo + EventRepo + Clone + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        config: WanMonitorConfig,
        state: watch::Sender<Arc<WanMonitorState>>,
        poke: Arc<Notify>,
    ) -> Self {
        Self {
            engine,
            repo,
            config,
            state,
            poke,
            outage: None,
        }
    }

    pub(super) async fn run(mut self, mut stopped: oneshot::Receiver<()>) {
        match self.repo.raised_conditions().await {
            Ok(raised) => {
                self.outage = raised
                    .into_iter()
                    .find(|(key, _)| key == OUTAGE_CONDITION)
                    .map(|(_, at)| (WanFault::Unknown, at));
            }
            Err(err) => warn!(%err, "WAN monitor: cannot read the conditions still raised"),
        }
        let mut tick = tokio::time::interval(self.config.every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let poke = Arc::clone(&self.poke);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                () = poke.notified() => self.round().await,
                _ = tick.tick() => self.round().await,
            }
        }
    }

    async fn settings(&self) -> WanMonitorSettings {
        match self.repo.setting(WanMonitorSettings::KEY).await {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_else(|err| {
                warn!(%err, "WAN monitor: the saved settings don't read; using the defaults");
                WanMonitorSettings::default()
            }),
            Ok(None) => WanMonitorSettings::default(),
            Err(err) => {
                warn!(%err, "WAN monitor: cannot read the settings; using the defaults");
                WanMonitorSettings::default()
            }
        }
    }

    /// The gateway, while it is online.
    fn gateway(&self) -> Result<NodeId, String> {
        let topology = &self.engine.context().topology;
        let (node, view) = self
            .engine
            .all()
            .into_iter()
            .find(|&(node, _)| topology.role(node) == Some(NodeRole::Gateway))
            .ok_or_else(|| "No gateway is adopted".to_owned())?;
        if view.status.state == NodeState::Offline {
            return Err("The gateway is offline".to_owned());
        }
        Ok(node)
    }

    async fn round(&mut self) {
        let settings = self.settings().await;
        let previous = self.state.borrow().clone();
        let mut state = WanMonitorState {
            settings: settings.clone(),
            ..WanMonitorState::default()
        };
        if !settings.enabled {
            self.end_outage(self.engine.context().clock.now());
            self.state.send_replace(Arc::new(state));
            return;
        }
        let done = match self.gateway() {
            Ok(node) => {
                state.gateway = Some(node);
                self.probe(node, &settings).await
            }
            Err(error) => Err(error),
        };
        let round = match done {
            Ok(round) => round,
            Err(error) => {
                // Nothing is concluded: keep what was known.
                state.error = Some(error);
                state.health = previous.health;
                state.since = previous.since;
                state.last_hop = previous.last_hop;
                self.state.send_replace(Arc::new(state));
                return;
            }
        };
        self.record(&round).await;
        let health = assess(&settings, &round);
        state.since = if previous.health == Some(health) {
            previous.since
        } else {
            Some(round.at)
        };
        state.health = Some(health);
        match health {
            WanHealth::Down(fault) => {
                if self.outage.is_none() {
                    let last_hop = self.trace(state.gateway, &settings, &round).await;
                    self.start_outage(fault, last_hop, round.at);
                    state.last_hop = last_hop;
                } else {
                    state.last_hop = previous.last_hop;
                }
            }
            WanHealth::Up | WanHealth::Unknown => self.end_outage(round.at),
        }
        state.round = Some(round);
        self.state.send_replace(Arc::new(state));
    }

    async fn probe(
        &self,
        node: NodeId,
        settings: &WanMonitorSettings,
    ) -> Result<ProbeRound, String> {
        let link = self
            .engine
            .link(node)
            .ok_or_else(|| "The gateway isn't watched".to_owned())?;
        let at = self.engine.context().clock.now();
        let output = link
            .exec(probe::round(settings))
            .await
            .map_err(|err| err.to_string())?;
        probe::parse_round(at, output).map_err(|err| err.to_string())
    }

    /// Stores the round's samples and announces them to the alert rules.
    async fn record(&self, round: &ProbeRound) {
        let mut samples = Vec::new();
        for result in &round.results {
            let entity = Entity::Probe(result.key.clone());
            let mut sample = |metric, value| {
                samples.push(Sample {
                    entity: entity.clone(),
                    metric,
                    at: round.at,
                    value,
                });
            };
            if let Some((min, median, max)) = result.band() {
                sample(Metric::Rtt, median);
                sample(Metric::RttMin, min);
                sample(Metric::RttMax, max);
            }
            if result.sent > 0 {
                sample(Metric::Loss, result.loss_percent());
            }
        }
        if samples.is_empty() {
            return;
        }
        if let Err(err) = self.repo.record_metrics(samples.clone()).await {
            warn!(%err, "WAN monitor: cannot store the round");
            return;
        }
        self.engine.context().publish(Event::Sampled {
            samples: samples.into(),
        });
    }

    /// The furthest hop toward the first internet target that still answers, when the line
    /// isn't down at the gateway.
    async fn trace(
        &self,
        gateway: Option<NodeId>,
        settings: &WanMonitorSettings,
        round: &ProbeRound,
    ) -> Option<IpAddr> {
        if !round.route {
            return None;
        }
        let host = settings
            .targets
            .iter()
            .find_map(|target| match &target.probe {
                Probe::Icmp { host: Some(host) } => Some(host.clone()),
                _ => None,
            })?;
        let link = self.engine.link(gateway?)?;
        match link.exec(probe::traceroute(&host)).await {
            Ok(output) => probe::last_hop(&String::from_utf8_lossy(&output.stdout)),
            Err(err) => {
                warn!(%err, "WAN monitor: the traceroute failed");
                None
            }
        }
    }

    fn start_outage(&mut self, fault: WanFault, last_hop: Option<IpAddr>, at: Timestamp) {
        self.outage = Some((fault, at));
        let what = EventKind::WanOutage {
            fault,
            last_hop: last_hop.map(|hop| hop.to_string()),
        };
        self.engine.note(
            NewEvent::new(at, what)
                .condition(OUTAGE_CONDITION, true)
                .fold(OUTAGE_CONDITION),
        );
    }

    fn end_outage(&mut self, at: Timestamp) {
        let Some((fault, since)) = self.outage.take() else {
            return;
        };
        let down_ms = u64::try_from(at.saturating_duration_since(since).as_millis()).ok();
        let what = EventKind::WanRestored { fault, down_ms };
        self.engine
            .note(NewEvent::new(at, what).condition(OUTAGE_CONDITION, false));
    }
}

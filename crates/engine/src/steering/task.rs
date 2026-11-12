//! The steering task: whom to measure, whom to ask, and the requests in flight.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, warn};
use uniwrt_core::{JobId, MacAddr, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    job::{Job, JobState},
    site::{SiteConfig, steering::SteeringPolicy},
    status::NodeState,
    wifi::Band,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    proto::ops::{Btm, BtmSent, MboReason, code, usteer_client_info, usteer_heard},
};
use uniwrt_store::RevisionRepo;

use super::{
    SteeringConfig,
    decide::{self, Place, Station, Target},
};
use crate::{Engine, neighbors::OwnReports, wlans::next_job};

/// What steering knows of a client it measured.
struct Client {
    /// The access point it was on: node and interface.
    at: (NodeId, String),
    /// Since when it is heard above band steering's signal on 2.4 GHz.
    strong_since: Option<Timestamp>,
    /// Not asked again before this.
    next: Timestamp,
    running: bool,
}

/// A request to send.
enum Ask {
    /// To these access points of the same node.
    Band(Vec<String>),
    /// To where usteer hears it better.
    Roam,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    own: watch::Receiver<Arc<OwnReports>>,
    config: SteeringConfig,
    head: Option<(RevisionId, Arc<SiteConfig>)>,
    /// The nodes each running job holds.
    jobs: BTreeMap<JobId, Vec<NodeId>>,
    clients: HashMap<MacAddr, Client>,
    done: (mpsc::Sender<MacAddr>, mpsc::Receiver<MacAddr>),
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RevisionRepo + Clone + 'static,
{
    pub(super) fn new(
        engine: Engine<L>,
        repo: R,
        own: watch::Receiver<Arc<OwnReports>>,
        config: SteeringConfig,
    ) -> Self {
        Self {
            engine,
            repo,
            own,
            config,
            head: None,
            jobs: BTreeMap::new(),
            clients: HashMap::new(),
            done: mpsc::channel(32),
        }
    }

    pub(super) async fn run(
        mut self,
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
                Some(mac) = self.done.1.recv() => {
                    if let Some(client) = self.clients.get_mut(&mac) {
                        client.running = false;
                    }
                }
                _ = tick.tick() => {
                    self.reload().await;
                    self.measure();
                }
            }
        }
    }

    /// Reads the head revision when it moved.
    async fn reload(&mut self) {
        match self.repo.head_revision().await {
            Ok(Some(head)) if self.head.as_ref().is_none_or(|(id, _)| *id != head.meta.id) => {
                self.head = Some((head.meta.id, Arc::new(head.config)));
            }
            Ok(_) => {}
            Err(err) => warn!(%err, "steering: cannot read the site"),
        }
    }

    fn on_job(&mut self, job: &Job) {
        if job.state == JobState::Running {
            let nodes = job.node.into_iter().chain(job.nodes.iter().map(|n| n.node));
            self.jobs.insert(job.id, nodes.collect());
        } else {
            self.jobs.remove(&job.id);
        }
    }

    fn busy(&self, node: NodeId) -> bool {
        self.jobs.values().any(|nodes| nodes.contains(&node))
    }

    /// Measures the clients of the nodes where usteer runs and asks those due.
    fn measure(&mut self) {
        let now = self.engine.context().clock.now();
        let Some((_, site)) = &self.head else {
            return;
        };
        let site = Arc::clone(site);
        let Some(policy) = site.steering.profile.policy() else {
            self.clients.retain(|_, client| client.running);
            return;
        };
        let every = Duration::from_secs(policy.every_s.into());
        let ssids = site.steered_ssids();
        let mut present = HashSet::new();
        let mut asks = Vec::new();
        for config in &site.nodes {
            let node = config.node;
            let Some(view) = self.engine.node(node) else {
                continue;
            };
            let Some(facts) = view
                .facts
                .filter(|_| view.status.state != NodeState::Offline)
            else {
                continue;
            };
            if self.busy(node) || !site.steers(config, &facts) {
                continue;
            }
            for station in decide::stations(&facts, &ssids) {
                present.insert(station.mac);
                let at = (node, station.iface.clone());
                let client = self.clients.entry(station.mac).or_insert_with(|| Client {
                    at: at.clone(),
                    strong_since: None,
                    next: now,
                    running: false,
                });
                if client.at != at {
                    client.at = at;
                    client.strong_since = None;
                }
                if station.band == Band::G2 && station.signal_dbm >= policy.band_above_dbm {
                    client.strong_since.get_or_insert(now);
                } else {
                    client.strong_since = None;
                }
                if client.running || now < client.next {
                    continue;
                }
                let strong_long = client
                    .strong_since
                    .is_some_and(|since| now.saturating_duration_since(since) >= every);
                let ask = if strong_long {
                    let targets = decide::upper_bands(&facts, &station.ssid);
                    (!targets.is_empty()).then_some(Ask::Band(targets))
                } else if policy
                    .roam_below_dbm
                    .is_some_and(|below| station.signal_dbm < below)
                {
                    Some(Ask::Roam)
                } else {
                    None
                };
                if let Some(ask) = ask {
                    client.running = true;
                    client.next = now.saturating_add(every);
                    asks.push((node, station, ask));
                }
            }
        }
        self.clients
            .retain(|mac, client| client.running || present.contains(mac));
        if asks.is_empty() {
            return;
        }
        let places: Arc<[Place]> = site
            .nodes
            .iter()
            .filter_map(|c| Some(Place::of(c.node, self.engine.node(c.node)?.facts?.as_ref())))
            .collect();
        for (node, station, ask) in asks {
            self.ask(node, station, ask, policy, Arc::clone(&places));
        }
    }

    fn ask(
        &mut self,
        node: NodeId,
        station: Station,
        ask: Ask,
        policy: SteeringPolicy,
        places: Arc<[Place]>,
    ) {
        let mac = station.mac;
        let Some(link) = self.engine.link(node) else {
            if let Some(client) = self.clients.get_mut(&mac) {
                client.running = false;
            }
            return;
        };
        let own = Arc::clone(&self.own.borrow());
        let (done, config) = (self.done.0.clone(), self.config.clone());
        tokio::spawn(async move {
            let request = Request {
                node,
                station: &station,
                policy: &policy,
                places: &places,
                own: &own,
                config: &config,
            };
            match request.send(&*link, ask).await {
                Ok(Some(sent)) => {
                    debug!(%node, %mac, iface = %sent.ifname, token = sent.token, "steering: asked");
                }
                Ok(None) => debug!(%node, %mac, "steering: no better access point"),
                // It left, or the node can't (the basic wpad): nothing to say.
                Err(LinkError::Remote { code: c, .. })
                    if c == code::NOT_FOUND || c == code::UNSUPPORTED =>
                {
                    debug!(%node, %mac, code = %c, "steering: not asked");
                }
                Err(err) => warn!(%node, %mac, %err, "steering: the request failed"),
            }
            let _ = done.send(mac).await;
        });
    }
}

/// One client's request, with what choosing its candidates needs.
struct Request<'a> {
    node: NodeId,
    station: &'a Station,
    policy: &'a SteeringPolicy,
    places: &'a [Place],
    own: &'a OwnReports,
    config: &'a SteeringConfig,
}

impl Request<'_> {
    /// Sends the request; `None` when a roam suggestion finds no better access point.
    async fn send<L: NodeLink>(&self, link: &L, ask: Ask) -> Result<Option<BtmSent>, LinkError> {
        let timeout = self.config.timeout;
        let (targets, reason, imminent, kick_ms): (Vec<Target>, _, _, _) = match ask {
            Ask::Band(ifaces) => {
                let targets = ifaces.into_iter().map(|i| (self.node, i)).collect();
                (targets, MboReason::PremiumAp, false, 0)
            }
            Ask::Roam => {
                let request = usteer_client_info(self.station.mac);
                let reply = link.request(&request, timeout).await?;
                let heard = usteer_heard(&reply)
                    .map_err(|err| LinkError::Protocol(format!("usteer's client info: {err}")))?;
                let targets =
                    decide::roam_targets(self.node, self.station, &heard, self.places, self.policy);
                if targets.is_empty() {
                    return Ok(None);
                }
                (targets, MboReason::LowSignal, true, self.config.kick_ms)
            }
        };
        let btm = Btm {
            addr: self.station.mac.to_string(),
            neighbors: decide::candidates(&targets, self.own),
            reason,
            imminent,
            kick_ms,
            reassoc_s: 0,
        };
        link.request(&btm, timeout).await.map(Some)
    }
}

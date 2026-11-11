//! The fleet at startup: an SSH link per adopted node, handed to the engine with the node's
//! latest snapshot, the snapshot writer, the topology's history, the metrics recorder, the
//! clients' recorder (which restores the clients first) and the log recorder (with the syslog
//! receiver, when it listens); the event recorder, the notifications, the alert engine and RF
//! insight's scanner join them once the job runner is up ([`Fleet::record_events`]; demo mode
//! scans sooner and busy or not). In demo mode the nodes are simulated
//! instead,
//! adopted into the demo's own database but for the last access point, which waits to be
//! adopted over [`DemoDial`]. The job runner hands nodes adopted later to the engine
//! ([`Watcher`]).

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use tracing::warn;
use uniwrt_core::Clock;
use uniwrt_engine::{
    AlertEngine, Alerts, AlertsConfig, Bans, BansConfig, ClientBans, ClientRecorder, DriftConfig,
    DriftWatch, Drifts, Engine, EngineConfig, EventRecorder, EventsConfig, HistoryConfig,
    LogRecorder, LogsConfig, MetricsConfig, MetricsRecorder, Notifications, Notify, NotifyConfig,
    RecorderConfig, Rf, RfConfig, RfScanner, SchedulesConfig, Snapshots, TopologyHistory,
    WlanSchedules,
};
use uniwrt_jobs::{BoxFuture, Dial, Jobs, JobsConfig, SshDial, Watch};
use uniwrt_model::{
    facts::NodeFacts,
    node::{NewNode, Node},
};
use uniwrt_node::{AgentLaunch, ControllerKey, Credential, HostKey, LinkConfig, SshLink, Target};
use uniwrt_sim::{SimConfig, SimFleet};
use uniwrt_store::{FactsRepo, HostKeyRepo, NodeRepo, Store, Vault};
use uniwrt_syslog::{Receiver, ReceiverConfig};

use crate::{demo::DemoDial, fleet_link::FleetLink};

/// How often each node's facts are saved: recent enough for a restart to show them, rarely
/// enough to spare the disk (D-037).
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(300);
/// How long stopping waits for the links to disconnect.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// The simulated nodes left for the user to adopt, the last ones.
const DEMO_UNADOPTED: usize = 1;

pub(crate) struct Fleet {
    pub(crate) engine: Engine<FleetLink>,
    snapshots: Snapshots,
    history: TopologyHistory,
    clients: ClientRecorder,
    metrics: MetricsRecorder,
    logs: LogRecorder,
    events: Option<EventRecorder>,
    alerts: Option<AlertEngine>,
    /// The notifications, and the vault that keeps notifiers' secrets.
    notifications: Option<(Notifications, Vault)>,
    /// RF insight's scanner.
    rf: Option<RfScanner>,
    /// Drift detection.
    drift: Option<DriftWatch>,
    schedules: Option<WlanSchedules>,
    bans: Option<ClientBans>,
    syslog: Option<Receiver>,
    /// Demo mode's world, which runs while it lives, and the dial that reaches its nodes.
    sim: Option<(SimFleet, Arc<DemoDial>)>,
    /// The simulated nodes demo mode adopted, in the sim's order.
    demo_adopted: Vec<uniwrt_core::NodeId>,
}

impl Fleet {
    /// Connects to every adopted node. A node that cannot be connected to is skipped with a
    /// warning; the others are not held up by it. Syslog is received at `syslog` when given.
    pub(crate) async fn start(
        store: &Store,
        key: &ControllerKey,
        clock: Arc<dyn Clock>,
        syslog: Option<std::net::SocketAddr>,
    ) -> anyhow::Result<Self> {
        let syslog = receive_syslog(syslog, &clock).await?;
        let engine = Engine::new(EngineConfig::default(), Arc::clone(&clock));
        let clients =
            ClientRecorder::start(&engine, store.clone(), RecorderConfig::default()).await;
        for node in store.nodes().await.context("cannot read the nodes")? {
            let id = node.id;
            match link(store, &node, key, &clock).await {
                Ok(link) => {
                    let last = store.latest_facts(id).await.unwrap_or_else(|err| {
                        warn!(node_id = %id, %err, "cannot read the node's last facts");
                        None
                    });
                    engine
                        .add(id, (&node).into(), FleetLink::Ssh(link), last)
                        .await;
                }
                Err(err) => warn!(node_id = %id, "not watching the node: {err:#}"),
            }
        }
        let snapshots = Snapshots::spawn(&engine, store.clone(), SNAPSHOT_INTERVAL);
        let history = TopologyHistory::spawn(&engine, store.clone(), HistoryConfig::default());
        let metrics = MetricsRecorder::spawn(&engine, store.clone(), MetricsConfig::default());
        let (syslog, received) = syslog.unzip();
        let logs =
            LogRecorder::start(&engine, store.clone(), received, LogsConfig::default()).await;
        Ok(Self {
            engine,
            snapshots,
            history,
            clients,
            metrics,
            logs,
            events: None,
            alerts: None,
            notifications: None,
            rf: None,
            drift: None,
            schedules: None,
            bans: None,
            syslog,
            sim: None,
            demo_adopted: Vec::new(),
        })
    }

    /// Simulates the fleet `config` describes, adopting all its nodes but the last (none of a
    /// factory-default gateway's fleet) into
    /// `store`, which should be empty (the demo's own), as adoption would: their host keys
    /// pinned and `controller_key` authorized on them, so management and applies reach them.
    /// Syslog is received at `syslog` when given.
    pub(crate) async fn demo(
        store: &Store,
        config: &SimConfig,
        controller_key: &ControllerKey,
        clock: Arc<dyn Clock>,
        syslog: Option<std::net::SocketAddr>,
    ) -> anyhow::Result<Self> {
        let syslog = receive_syslog(syslog, &clock).await?;
        let sim = SimFleet::spawn(config, clock.as_ref());
        let dial = Arc::new(DemoDial::new(&sim).context("cannot make the demo's host keys")?);
        let engine = Engine::new(EngineConfig::default(), Arc::clone(&clock));
        let clients =
            ClientRecorder::start(&engine, store.clone(), RecorderConfig::default()).await;
        let nodes = sim.nodes();
        // A factory-default gateway waits for the first-run wizard.
        let adopted = if config.factory {
            0
        } else {
            nodes.len().saturating_sub(DEMO_UNADOPTED).max(1)
        };
        let mut demo_adopted = Vec::with_capacity(adopted);
        for (index, node) in nodes.into_iter().enumerate().take(adopted) {
            let adopted = store
                .create_node(NewNode {
                    name: node.name,
                    mac: node.mac,
                    board_name: node.board_name,
                    host: node.host,
                    port: 22,
                    role: node.role,
                })
                .await
                .context("cannot adopt a simulated node")?;
            let link = sim.link(index).context("a simulated node has no link")?;
            let host_key = dial
                .host_key(&adopted.host)
                .context("a simulated node has no host key")?;
            store
                .pin_host_key(adopted.id, host_key.pin(clock.now()))
                .await
                .context("cannot pin a simulated node's host key")?;
            link.authorize(&controller_key.authorized_key()?);
            demo_adopted.push(adopted.id);
            engine
                .add(adopted.id, (&adopted).into(), FleetLink::Sim(link), None)
                .await;
        }
        let snapshots = Snapshots::spawn(&engine, store.clone(), SNAPSHOT_INTERVAL);
        let history = TopologyHistory::spawn(&engine, store.clone(), HistoryConfig::default());
        let metrics = MetricsRecorder::spawn(&engine, store.clone(), MetricsConfig::default());
        let (syslog, received) = syslog.unzip();
        let logs =
            LogRecorder::start(&engine, store.clone(), received, LogsConfig::default()).await;
        Ok(Self {
            engine,
            snapshots,
            history,
            clients,
            metrics,
            logs,
            events: None,
            alerts: None,
            notifications: None,
            rf: None,
            drift: None,
            schedules: None,
            bans: None,
            syslog,
            sim: Some((sim, dial)),
            demo_adopted,
        })
    }

    /// Demo mode's first revision: the import of the simulated nodes it adopted (`None` outside
    /// demo mode, and when it adopted none).
    pub(crate) fn first_site(&self) -> anyhow::Result<Option<uniwrt_api::FirstSite>> {
        self.sim
            .as_ref()
            .filter(|_| !self.demo_adopted.is_empty())
            .map(|(sim, _)| crate::demo_site::first_site(&sim.brownfield(), &self.demo_adopted))
            .transpose()
    }

    /// The simulated node at `host`, in demo mode.
    #[cfg(test)]
    pub(crate) fn sim_link(&self, host: &str) -> Option<uniwrt_sim::SimLink> {
        self.sim.as_ref()?.1.link(host)
    }

    /// Stops the actors, saves the last snapshots and disconnects.
    pub(crate) async fn stop(self) {
        let links: Vec<_> = self
            .engine
            .all()
            .into_iter()
            .filter_map(|(id, _)| self.engine.link(id))
            .collect();
        if let Some(syslog) = self.syslog {
            syslog.stop().await;
        }
        self.engine.shutdown().await;
        if let Some(events) = self.events {
            events.stop().await;
        }
        if let Some(alerts) = self.alerts {
            alerts.stop().await;
        }
        if let Some((notifications, _)) = self.notifications {
            notifications.stop().await;
        }
        if let Some(rf) = self.rf {
            rf.stop().await;
        }
        if let Some(schedules) = self.schedules {
            schedules.stop().await;
        }
        if let Some(bans) = self.bans {
            bans.stop().await;
        }
        if let Some(drift) = self.drift {
            drift.stop().await;
        }
        self.logs.stop().await;
        self.snapshots.stop().await;
        self.history.stop().await;
        self.clients.stop().await;
        self.metrics.stop().await;
        drop(self.sim);
        let disconnect = async {
            for link in &links {
                link.shutdown().await;
            }
        };
        if tokio::time::timeout(DISCONNECT_TIMEOUT, disconnect)
            .await
            .is_err()
        {
            warn!("links still connected after {DISCONNECT_TIMEOUT:?}");
        }
    }

    /// Starts recording events, from the engine and from `jobs` (the controller's start is the
    /// first), evaluating alerts over them, and notifying of the alerts (started first, so it
    /// hears of what the alert engine stores at its start).
    pub(crate) async fn record_events(&mut self, store: &Store, vault: &Vault, jobs: &Jobs) {
        let config = EventsConfig {
            started: Some(crate::build_info::VERSION.to_owned()),
            ..EventsConfig::default()
        };
        let recorder =
            EventRecorder::start(&self.engine, store.clone(), Some(jobs.subscribe()), config).await;
        self.events = Some(recorder);
        match uniwrt_notify::Sender::new(&format!("uniwrt/{}", crate::build_info::VERSION)) {
            Ok(sender) => {
                let notifications = Notifications::start(
                    &self.engine,
                    store.clone(),
                    vault.clone(),
                    sender,
                    NotifyConfig::default(),
                )
                .await;
                self.notifications = Some((notifications, vault.clone()));
            }
            Err(err) => warn!(%err, "notifications cannot start"),
        }
        let alerts = AlertEngine::start(
            &self.engine,
            store.clone(),
            Some(jobs.subscribe()),
            AlertsConfig::default(),
        )
        .await;
        self.alerts = Some(alerts);
        let rf = if self.sim.is_some() {
            // The demo shows RF insight within its first minute; its traffic never pauses.
            RfConfig {
                first_delay: Duration::from_secs(5),
                stagger: Duration::from_secs(3),
                gap: Duration::from_secs(2),
                idle_kbps: u64::MAX,
                ..RfConfig::default()
            }
        } else {
            RfConfig::default()
        };
        self.rf = Some(RfScanner::start(&self.engine, store.clone(), rf).await);
        match DriftWatch::start(
            &self.engine,
            store.clone(),
            Some(jobs.subscribe()),
            DriftConfig::default(),
        )
        .await
        {
            Ok(drift) => self.drift = Some(drift),
            Err(err) => warn!(%err, "drift detection cannot start"),
        }
        self.schedules = Some(WlanSchedules::start(
            &self.engine,
            store.clone(),
            Some(jobs.subscribe()),
            SchedulesConfig::default(),
        ));
        self.bans = Some(ClientBans::start(
            &self.engine,
            store.clone(),
            Some(jobs.subscribe()),
            BansConfig::default(),
        ));
    }

    /// Drift detection's handle, once [`Fleet::record_events`] started it.
    pub(crate) fn drift(&self) -> Option<Drifts> {
        self.drift.as_ref().map(DriftWatch::handle)
    }

    /// The client bans' handle, once [`Fleet::record_events`] started them.
    pub(crate) fn bans(&self) -> Option<Bans> {
        self.bans.as_ref().map(ClientBans::handle)
    }

    /// RF insight's handle, once [`Fleet::record_events`] started it.
    pub(crate) fn rf(&self) -> Option<Rf> {
        self.rf.as_ref().map(RfScanner::handle)
    }

    /// The alert engine's handle, once [`Fleet::record_events`] started it.
    pub(crate) fn alerts(&self) -> Option<Alerts> {
        self.alerts.as_ref().map(AlertEngine::handle)
    }

    /// The notifications' handle and the vault, once [`Fleet::record_events`] started them.
    pub(crate) fn notify(&self) -> Option<(Notify, Vault)> {
        self.notifications
            .as_ref()
            .map(|(notifications, vault)| (notifications.handle(), vault.clone()))
    }

    /// The job runner over this fleet: the nodes it adopts join the engine. Demo mode adopts
    /// the simulated nodes.
    pub(crate) async fn jobs(
        &self,
        store: &Store,
        vault: Vault,
        key: &ControllerKey,
        clock: Arc<dyn Clock>,
    ) -> anyhow::Result<Jobs> {
        let demo = self.sim.as_ref().map(|(_, dial)| Arc::clone(dial));
        let dial = match &demo {
            Some(demo) => Arc::clone(demo) as Arc<dyn Dial>,
            None => Arc::new(SshDial::default()),
        };
        Jobs::start(JobsConfig {
            store: store.clone(),
            vault,
            clock: Arc::clone(&clock),
            key: key.clone(),
            dial: Some(dial),
            watch: Arc::new(Watcher {
                engine: self.engine.clone(),
                key: key.clone(),
                clock,
                demo,
            }),
        })
        .await
        .context("cannot start the jobs")
    }
}

/// The syslog receiver on UDP and TCP at `addr`, when given.
async fn receive_syslog(
    addr: Option<std::net::SocketAddr>,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<
    Option<(
        Receiver,
        tokio::sync::mpsc::Receiver<uniwrt_syslog::Received>,
    )>,
> {
    let Some(addr) = addr else {
        return Ok(None);
    };
    let config = ReceiverConfig {
        udp: Some(addr),
        tcp: Some(addr),
        ..ReceiverConfig::default()
    };
    let receiver = Receiver::bind(&config, Arc::clone(clock))
        .await
        .with_context(|| format!("cannot listen for syslog on {addr}"))?;
    tracing::info!(%addr, "receiving syslog over UDP and TCP");
    Ok(Some(receiver))
}

/// Watches each node the job runner adopts over an SSH link, as [`Fleet::start`] does with the
/// nodes adopted before; in demo mode, over the simulated node's link.
pub(crate) struct Watcher {
    pub(crate) engine: Engine<FleetLink>,
    pub(crate) key: ControllerKey,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) demo: Option<Arc<DemoDial>>,
}

impl Watcher {
    fn link(&self, node: &Node, host_key: HostKey) -> FleetLink {
        match self.demo.as_ref().and_then(|demo| demo.link(&node.host)) {
            Some(link) => FleetLink::Sim(link),
            None => FleetLink::Ssh(ssh_link(node, host_key, &self.key, &self.clock)),
        }
    }
}

impl Watch for Watcher {
    fn watch(&self, node: Node, host_key: HostKey, facts: NodeFacts) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let link = self.link(&node, host_key);
            self.engine
                .add(node.id, (&node).into(), link, Some(facts))
                .await;
        })
    }

    fn moved(&self, node: Node, host_key: HostKey) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let link = self.link(&node, host_key);
            self.engine.add(node.id, (&node).into(), link, None).await;
        })
    }
}

async fn link(
    store: &Store,
    node: &Node,
    key: &ControllerKey,
    clock: &Arc<dyn Clock>,
) -> anyhow::Result<SshLink> {
    let pin = store
        .host_key(node.id)
        .await?
        .context("no host key is pinned for it")?;
    let host_key = HostKey::from_pin(&pin).context("its pinned host key is invalid")?;
    Ok(ssh_link(node, host_key, key, clock))
}

/// A link to the node with the controller's key, which runs the agent's bundle.
fn ssh_link(
    node: &Node,
    host_key: HostKey,
    key: &ControllerKey,
    clock: &Arc<dyn Clock>,
) -> SshLink {
    let mut config = LinkConfig::new(
        Target::new(node.host.clone(), node.port),
        host_key,
        Credential::Key(key.clone()),
    );
    config.agent = Some(AgentLaunch::Bundle);
    SshLink::spawn(config, Arc::clone(clock))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::net::TcpListener;
    use uniwrt_core::{MacAddr, SystemClock};
    use uniwrt_model::{
        node::{NewNode, NodeRole},
        status::NodeState,
    };
    use uniwrt_node::{ControllerKey, HostKey};
    use uniwrt_store::{HostKeyRepo, NodeRepo, Store};

    use super::Fleet;

    fn new_node(last_octet: u8, port: u16) -> NewNode {
        NewNode {
            name: format!("ap{last_octet}"),
            mac: MacAddr::new([2, 0, 0, 0, 0, last_octet]),
            board_name: "hwsim".to_owned(),
            host: "127.0.0.1".to_owned(),
            port,
            role: NodeRole::Ap,
        }
    }

    #[tokio::test]
    async fn pinned_nodes_are_watched_and_unpinned_ones_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(SystemClock);
        let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
            .await
            .unwrap();
        // Nothing listens on the port: the node stays offline.
        let port = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let key = ControllerKey::generate().unwrap();
        let pinned = store.create_node(new_node(1, port)).await.unwrap();
        let host_key =
            HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap()).unwrap();
        store
            .pin_host_key(pinned.id, host_key.pin(pinned.created_at))
            .await
            .unwrap();
        let unpinned = store.create_node(new_node(2, port)).await.unwrap();

        let fleet = Fleet::start(&store, &key, clock, None).await.unwrap();
        let view = fleet.engine.node(pinned.id).unwrap();
        assert_eq!(view.status.state, NodeState::Offline);
        assert!(fleet.engine.node(unpinned.id).is_none());
        tokio::time::timeout(Duration::from_secs(5), fleet.stop())
            .await
            .unwrap();
        store.close().await;
    }

    #[tokio::test]
    async fn the_demo_adopts_and_watches_all_simulated_nodes_but_the_last() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(SystemClock);
        let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
            .await
            .unwrap();
        // Syslog on a free port of loopback.
        let syslog = "127.0.0.1:0".parse().ok();
        let key = ControllerKey::generate().unwrap();
        let fleet = Fleet::demo(
            &store,
            &uniwrt_sim::SimConfig::default(),
            &key,
            clock,
            syslog,
        )
        .await
        .unwrap();
        assert!(
            fleet
                .syslog
                .as_ref()
                .and_then(uniwrt_syslog::Receiver::udp_addr)
                .is_some()
        );
        // The last access point waits to be adopted (`demo::tests`).
        let nodes = store.nodes().await.unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].role, NodeRole::Gateway);
        let online = || {
            nodes.iter().all(|node| {
                fleet.engine.node(node.id).map(|view| view.status.state) == Some(NodeState::Online)
            })
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            while !online() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the simulated nodes come online");
        tokio::time::timeout(Duration::from_secs(5), fleet.stop())
            .await
            .unwrap();
        store.close().await;
    }
}

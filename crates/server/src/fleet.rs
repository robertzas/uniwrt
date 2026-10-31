//! The fleet at startup: an SSH link per adopted node, handed to the engine with the node's
//! latest snapshot, and the snapshot writer. In demo mode the nodes are simulated instead,
//! adopted into the demo's own database but for the last access point, which waits to be
//! adopted over [`DemoDial`]. The job runner hands nodes adopted later to the engine
//! ([`Watcher`]).

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use tracing::warn;
use uniwrt_core::Clock;
use uniwrt_engine::{Engine, EngineConfig, Snapshots};
use uniwrt_jobs::{BoxFuture, Dial, Jobs, JobsConfig, SshDial, Watch};
use uniwrt_model::{
    facts::NodeFacts,
    node::{NewNode, Node},
};
use uniwrt_node::{AgentLaunch, ControllerKey, Credential, HostKey, LinkConfig, SshLink, Target};
use uniwrt_sim::{SimConfig, SimFleet};
use uniwrt_store::{FactsRepo, HostKeyRepo, NodeRepo, Store, Vault};

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
    /// Demo mode's world, which runs while it lives, and the dial that reaches its nodes.
    sim: Option<(SimFleet, Arc<DemoDial>)>,
}

impl Fleet {
    /// Connects to every adopted node. A node that cannot be connected to is skipped with a
    /// warning; the others are not held up by it.
    pub(crate) async fn start(
        store: &Store,
        key: &ControllerKey,
        clock: Arc<dyn Clock>,
    ) -> anyhow::Result<Self> {
        let engine = Engine::new(EngineConfig::default(), Arc::clone(&clock));
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
        Ok(Self {
            engine,
            snapshots,
            sim: None,
        })
    }

    /// Simulates the fleet `config` describes, adopting all its nodes but the last into
    /// `store`, which should be empty (the demo's own).
    pub(crate) async fn demo(
        store: &Store,
        config: &SimConfig,
        clock: Arc<dyn Clock>,
    ) -> anyhow::Result<Self> {
        let sim = SimFleet::spawn(config, clock.as_ref());
        let dial = Arc::new(DemoDial::new(&sim).context("cannot make the demo's host keys")?);
        let engine = Engine::new(EngineConfig::default(), clock);
        let nodes = sim.nodes();
        let adopted = nodes.len().saturating_sub(DEMO_UNADOPTED).max(1);
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
            engine
                .add(adopted.id, (&adopted).into(), FleetLink::Sim(link), None)
                .await;
        }
        let snapshots = Snapshots::spawn(&engine, store.clone(), SNAPSHOT_INTERVAL);
        Ok(Self {
            engine,
            snapshots,
            sim: Some((sim, dial)),
        })
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
        self.engine.shutdown().await;
        self.snapshots.stop().await;
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

/// Watches each node the job runner adopts over an SSH link, as [`Fleet::start`] does with the
/// nodes adopted before; in demo mode, over the simulated node's link.
pub(crate) struct Watcher {
    pub(crate) engine: Engine<FleetLink>,
    pub(crate) key: ControllerKey,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) demo: Option<Arc<DemoDial>>,
}

impl Watch for Watcher {
    fn watch(&self, node: Node, host_key: HostKey, facts: NodeFacts) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let link = match self.demo.as_ref().and_then(|demo| demo.link(&node.host)) {
                Some(link) => FleetLink::Sim(link),
                None => FleetLink::Ssh(ssh_link(&node, host_key, &self.key, &self.clock)),
            };
            self.engine
                .add(node.id, (&node).into(), link, Some(facts))
                .await;
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

        let fleet = Fleet::start(&store, &key, clock).await.unwrap();
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
        let fleet = Fleet::demo(&store, &uniwrt_sim::SimConfig::default(), clock)
            .await
            .unwrap();
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

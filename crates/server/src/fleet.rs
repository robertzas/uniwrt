//! The fleet at startup: an SSH link per adopted node, handed to the engine with the node's
//! latest snapshot, and the snapshot writer.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use tracing::warn;
use uniwrt_core::Clock;
use uniwrt_engine::{Engine, EngineConfig, Snapshots};
use uniwrt_model::node::Node;
use uniwrt_node::{AgentLaunch, ControllerKey, Credential, HostKey, LinkConfig, SshLink, Target};
use uniwrt_store::{FactsRepo, HostKeyRepo, NodeRepo, Store};

/// How often each node's facts are saved: recent enough for a restart to show them, rarely
/// enough to spare the disk (D-037).
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(300);
/// How long stopping waits for the links to disconnect.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct Fleet {
    pub(crate) engine: Engine<SshLink>,
    snapshots: Snapshots,
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
                    engine.add(id, link, last).await;
                }
                Err(err) => warn!(node_id = %id, "not watching the node: {err:#}"),
            }
        }
        let snapshots = Snapshots::spawn(&engine, store.clone(), SNAPSHOT_INTERVAL);
        Ok(Self { engine, snapshots })
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
    let mut config = LinkConfig::new(
        Target::new(node.host.clone(), node.port),
        host_key,
        Credential::Key(key.clone()),
    );
    config.agent = Some(AgentLaunch::Bundle);
    Ok(SshLink::spawn(config, Arc::clone(clock)))
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
}

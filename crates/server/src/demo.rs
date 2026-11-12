//! Demo mode's adoptions: [`DemoDial`] reaches the simulated nodes as [`SshDial`] reaches real
//! ones, so the real adopt job and discovery run against them. Each node has a dropbear banner,
//! a host key made at start, and the root password `demo` (a factory-default gateway,
//! `--demo-factory`, has none until adoption sets one); it runs what adoption runs through
//! its [`SimLink`]. A node answers at the addresses it holds now ([`SimLink::hosts`]), so a
//! gateway that is renumbered is reached at its old address and its new one while it bridges
//! them (D-081). Every answer takes about as long as over a LAN, so the wizard shows each
//! step. Other hosts refuse the connection, as the simulated clients would.
//!
//! [`SshDial`]: uniwrt_jobs::SshDial

use std::{io, time::Duration};

use uniwrt_jobs::{BoxFuture, Dial, Remote};
use uniwrt_node::{
    ControllerKey, Credential, Error, ExecOutput, ExecRequest, HostKey, NodeLink as _, Result,
    Target,
    proto::ops::{Collect, Collected, Op as _, Set},
};
use uniwrt_sim::{SimFleet, SimLink};

/// The simulated nodes' root password, as the demo's own login.
pub(crate) const ROOT_PASSWORD: &str = uniwrt_sim::ROOT_PASSWORD;

const BANNER: &str = "SSH-2.0-dropbear_2025.88";
const BANNER_DELAY: Duration = Duration::from_millis(150);
const PROBE_DELAY: Duration = Duration::from_millis(400);
const LOGIN_DELAY: Duration = Duration::from_millis(700);
const EXEC_DELAY: Duration = Duration::from_millis(300);
/// The agent's upload and start, then a collect.
const COLLECT_DELAY: Duration = Duration::from_millis(1200);
const BACKUP_DELAY: Duration = Duration::from_millis(1500);

/// One simulated node, as the dial reaches it.
struct DemoNode {
    link: SimLink,
    host_key: HostKey,
}

/// Reaches the simulated nodes.
pub(crate) struct DemoDial {
    nodes: Vec<DemoNode>,
}

impl DemoDial {
    /// Every node of `sim`, each with a host key of its own.
    pub(crate) fn new(sim: &SimFleet) -> anyhow::Result<Self> {
        let nodes = (0..sim.nodes().len())
            .filter_map(|index| sim.link(index))
            .map(|link| {
                let host_key = HostKey::parse(&ControllerKey::generate()?.authorized_key()?)?;
                Ok(DemoNode { link, host_key })
            })
            .collect::<Result<_>>()?;
        Ok(Self { nodes })
    }

    /// The link of the node at `host`, for the engine to watch once it is adopted.
    pub(crate) fn link(&self, host: &str) -> Option<SimLink> {
        self.node(host).map(|node| node.link.clone())
    }

    /// The host key of the node at `host`, which demo mode pins for the nodes it adopts at
    /// start.
    pub(crate) fn host_key(&self, host: &str) -> Option<&HostKey> {
        self.node(host).map(|node| &node.host_key)
    }

    fn node(&self, host: &str) -> Option<&DemoNode> {
        self.nodes
            .iter()
            .find(|node| node.link.hosts().iter().any(|held| held == host))
    }

    /// The node at `target` after `delay`, or at once the error of a host without an SSH
    /// server.
    async fn reach(&self, target: &Target, delay: Duration) -> Result<&DemoNode> {
        let node = self
            .node(&target.host)
            .filter(|_| target.port == 22)
            .ok_or_else(|| Error::Connect {
                target: target.to_string(),
                source: io::Error::new(io::ErrorKind::ConnectionRefused, "Connection refused"),
            })?;
        tokio::time::sleep(delay).await;
        Ok(node)
    }
}

impl Dial for DemoDial {
    fn banner<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            self.reach(target, BANNER_DELAY).await?;
            Ok(Some(BANNER.to_owned()))
        })
    }

    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(async move { Ok(self.reach(target, PROBE_DELAY).await?.host_key.clone()) })
    }

    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        Box::pin(async move {
            let node = self.reach(target, LOGIN_DELAY).await?;
            if node.host_key.fingerprint() != host_key.fingerprint() {
                return Err(Error::HostKeyMismatch {
                    target: target.to_string(),
                    presented: node.host_key.clone(),
                    pinned: host_key.clone(),
                });
            }
            let (accepted, method) = match credential {
                // An empty one is the login without a password, which only a factory-default
                // root accepts.
                Credential::Password(password) => {
                    (node.link.root_password_is(password), "password")
                }
                Credential::Key(key) => (node.link.trusts(&key.authorized_key()?), "publickey"),
            };
            if !accepted {
                return Err(Error::AuthRejected {
                    target: target.to_string(),
                    user: "root".to_owned(),
                    method,
                });
            }
            Ok(Box::new(DemoRemote(node.link.clone())) as Box<dyn Remote>)
        })
    }
}

/// A login on a simulated node.
struct DemoRemote(SimLink);

impl Remote for DemoRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move {
            let delay = if request.argv.first().is_some_and(|arg| arg == "sysupgrade") {
                BACKUP_DELAY
            } else {
                EXEC_DELAY
            };
            tokio::time::sleep(delay).await;
            self.0.exec(request).await
        })
    }

    fn collect<'a>(&'a self, sets: &'a [Set]) -> BoxFuture<'a, Result<Collected>> {
        Box::pin(async move {
            tokio::time::sleep(COLLECT_DELAY).await;
            let args = serde_json::value::to_raw_value(&Collect {
                sets: sets.to_vec(),
            })
            .map_err(|err| Error::Protocol(err.to_string()))?;
            let answer = self.0.call(Collect::NAME, &args, COLLECT_DELAY).await?;
            serde_json::from_str(answer.get())
                .map_err(|err| Error::Protocol(format!("unexpected answer to collect: {err}")))
        })
    }

    fn call<'a>(
        &'a self,
        op: &'static str,
        args: &'a serde_json::value::RawValue,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<serde_json::value::RawValue>>> {
        Box::pin(async move {
            tokio::time::sleep(EXEC_DELAY).await;
            self.0.call(op, args, timeout).await
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use uniwrt_core::SystemClock;
    use uniwrt_jobs::{AdoptRequest, Jobs};
    use uniwrt_model::{
        job::{Job, JobErrorCode, JobState},
        node::NodeMode,
        site::{
            SiteConfig,
            node::{ManagedPackage, NodeConfig},
            revision::{NewRevision, RevisionKind},
        },
        status::NodeState,
    };
    use uniwrt_node::{ControllerKey, Target};
    use uniwrt_sim::SimConfig;
    use uniwrt_store::{MasterKey, NodeRepo as _, RevisionRepo as _, Store, Vault, VaultRepo as _};
    use zeroize::Zeroizing;

    use super::ROOT_PASSWORD;
    use crate::fleet::Fleet;

    async fn finished(jobs: &Jobs, job: Job) -> Job {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let job = jobs.job(job.id).await.unwrap().unwrap();
                if job.state != JobState::Running {
                    return job;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the job finishes")
    }

    #[tokio::test]
    async fn the_last_access_point_is_discovered_and_adopted_with_the_demo_password() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(SystemClock);
        let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
            .await
            .unwrap();
        let config = SimConfig {
            reboot_every: None,
            ..SimConfig::default()
        };
        let key = uniwrt_node::ControllerKey::generate().unwrap();
        let fleet = Fleet::demo(&store, &config, &key, clock.clone(), None)
            .await
            .unwrap();
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let key = ControllerKey::generate().unwrap();
        let jobs = fleet.jobs(&store, vault, &key, clock).await.unwrap();

        // The gateway's leases and mesh show the last access point, with its banner and key.
        let report = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let adopted: Vec<_> = store
                    .nodes()
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|node| {
                        let facts = fleet.engine.node(node.id).and_then(|view| view.facts);
                        (node, facts)
                    })
                    .collect();
                let report = jobs.discover(&adopted, Duration::from_secs(3)).await;
                if !report.scanning && !report.nodes.is_empty() {
                    return report;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("the last access point is discovered");
        assert_eq!(report.nodes.len(), 1);
        let found = &report.nodes[0];
        assert_eq!(found.ssh.as_deref(), Some(super::BANNER));
        let host = found.ip.unwrap().to_string();
        let target = Target::new(host.clone(), 22);
        let host_key = jobs.probe(&target).await.unwrap();
        assert_eq!(
            Some(host_key.fingerprint()),
            found.host_key.as_ref().map(|key| key.fingerprint.as_str())
        );
        let request = |password: &str| AdoptRequest {
            target: target.clone(),
            host_key: host_key.clone(),
            password: Some(Zeroizing::new(password.to_owned())),
            remember_password: false,
            name: None,
            role: None,
        };

        let rejected = jobs.adopt(request("root"), None).await.unwrap();
        let rejected = finished(&jobs, rejected).await;
        assert_eq!(
            rejected.error.map(|err| err.code),
            Some(JobErrorCode::AuthRejected)
        );

        let adopted = jobs.adopt(request(ROOT_PASSWORD), None).await.unwrap();
        let adopted = finished(&jobs, adopted).await;
        assert_eq!(adopted.state, JobState::Succeeded, "{:?}", adopted.error);
        let id = adopted.node.unwrap();
        assert_eq!(store.nodes().await.unwrap().len(), 4);
        tokio::time::timeout(Duration::from_secs(5), async {
            while fleet.engine.node(id).map(|view| view.status.state) != Some(NodeState::Online) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the adopted node comes online");
        let link = fleet.sim_link(&host).unwrap();
        assert!(link.trusts(&key.authorized_key().unwrap()));

        // Management goes on and off through the simulated transaction tool.
        for (enable, mode) in [(true, NodeMode::Managed), (false, NodeMode::Observe)] {
            let job = finished(&jobs, jobs.manage(id, enable, None).await.unwrap()).await;
            assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
            assert_eq!(store.node(id).await.unwrap().unwrap().mode, mode);
        }

        jobs.shutdown().await;
        tokio::time::timeout(Duration::from_secs(5), fleet.stop())
            .await
            .unwrap();
        store.close().await;
    }

    /// The first-run wizard's path (features/adoption.md, Path E): the factory-default gateway
    /// is found and adopted without a password, managed, and renumbered to the site's plan.
    #[tokio::test]
    async fn a_factory_gateway_is_found_adopted_and_renumbered() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(SystemClock);
        let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
            .await
            .unwrap();
        let key = ControllerKey::generate().unwrap();
        let fleet = Fleet::demo(&store, &SimConfig::factory(7), &key, clock.clone(), None)
            .await
            .unwrap();
        assert_eq!(store.nodes().await.unwrap().len(), 0, "nothing is adopted");
        assert!(fleet.first_site().unwrap().is_none());
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let jobs = fleet
            .jobs(&store, vault.clone(), &key, clock)
            .await
            .unwrap();

        let factory = std::net::Ipv4Addr::new(192, 168, 1, 1);
        let found = jobs
            .find_gateways(&[std::net::Ipv4Addr::new(172, 17, 0, 1), factory])
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        let gateway = &found[0];
        assert!(gateway.blank && gateway.model.is_some() && gateway.adopted.is_none());
        assert_eq!(gateway.hostname.as_deref(), Some("OpenWrt"));

        let request = AdoptRequest {
            target: gateway.target.clone(),
            host_key: gateway.host_key.clone(),
            password: Some(Zeroizing::new(String::new())),
            remember_password: false,
            name: None,
            // The wizard says so: with no internet yet, nothing else tells a gateway.
            role: Some(uniwrt_model::node::NodeRole::Gateway),
        };
        let adopted = finished(&jobs, jobs.adopt(request, None).await.unwrap()).await;
        assert_eq!(adopted.state, JobState::Succeeded, "{:?}", adopted.error);
        let id = adopted.node.unwrap();
        let link = fleet.sim_link("192.168.1.1").unwrap();
        assert!(!link.root_password_is(""), "root has a password now");
        let kept = vault
            .secret(&format!("node/{id}/root_password"))
            .await
            .unwrap()
            .expect("the vault keeps it");
        assert!(link.root_password_is(std::str::from_utf8(&kept).unwrap()));

        let managed = finished(&jobs, jobs.manage(id, true, None).await.unwrap()).await;
        assert_eq!(managed.state, JobState::Succeeded, "{:?}", managed.error);

        // The site's defaults put the LAN at 10.0.0.1/24: the gateway bridges, the hosts
        // (this controller among them) move, and the old address goes.
        let mut config = SiteConfig::defaults(None);
        config.nodes.push(NodeConfig {
            managed: [
                ManagedPackage::System,
                ManagedPackage::Network,
                ManagedPackage::Wireless,
                ManagedPackage::Dhcp,
                ManagedPackage::Firewall,
            ]
            .into(),
            ..NodeConfig::new(id)
        });
        let revision = store
            .commit_revision(
                NewRevision {
                    base: None,
                    kind: RevisionKind::Defaults,
                    restored_from: None,
                    author_id: None,
                    author: None,
                    message: "first run".to_owned(),
                    config,
                },
                Vec::new(),
            )
            .await
            .unwrap();
        let apply = jobs.apply(revision.id, None).await.unwrap();
        let apply = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let job = jobs.job(apply.id).await.unwrap().unwrap();
                if job.state != JobState::Running {
                    return job;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .expect("the apply finishes");
        assert_eq!(apply.state, JobState::Succeeded, "{:?}", apply.error);
        assert_eq!(
            store.node(id).await.unwrap().unwrap().host,
            "10.0.0.1",
            "{apply:#?}"
        );
        assert_eq!(link.hosts(), ["10.0.0.1"]);
        assert!(fleet.sim_link("192.168.1.1").is_none());

        jobs.shutdown().await;
        tokio::time::timeout(Duration::from_secs(5), fleet.stop())
            .await
            .unwrap();
        store.close().await;
    }
}

//! Demo mode's adoptions: [`DemoDial`] reaches the simulated nodes as [`SshDial`] reaches real
//! ones, so the real adopt job and discovery run against them. Each node has a dropbear banner,
//! a host key made at start, and the root password `demo`; it runs what adoption runs through
//! its [`SimLink`]. Every answer takes about as long as over a LAN, so the wizard shows each
//! step. Other hosts refuse the connection, as the simulated clients would.
//!
//! [`SshDial`]: uniwrt_jobs::SshDial

use std::{io, time::Duration};

use uniwrt_jobs::{BoxFuture, Dial, Remote};
use uniwrt_node::{
    ControllerKey, Credential, Error, ExecOutput, ExecRequest, HostKey, NodeLink as _, Result,
    Target,
    proto::ops::{Collect, Collected, Op as _},
};
use uniwrt_sim::{SimFleet, SimLink};

/// The simulated nodes' root password, as the demo's own login.
pub(crate) const ROOT_PASSWORD: &str = "demo";

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
    host: String,
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
        let nodes = sim
            .nodes()
            .into_iter()
            .enumerate()
            .filter_map(|(index, node)| Some((node.host, sim.link(index)?)))
            .map(|(host, link)| {
                let host_key = HostKey::parse(&ControllerKey::generate()?.authorized_key()?)?;
                Ok(DemoNode {
                    host,
                    link,
                    host_key,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { nodes })
    }

    /// The link of the node at `host`, for the engine to watch once it is adopted.
    pub(crate) fn link(&self, host: &str) -> Option<SimLink> {
        self.node(host).map(|node| node.link.clone())
    }

    fn node(&self, host: &str) -> Option<&DemoNode> {
        self.nodes.iter().find(|node| node.host == host)
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
                Credential::Password(password) => (password.as_str() == ROOT_PASSWORD, "password"),
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

    fn collect(&self) -> BoxFuture<'_, Result<Collected>> {
        Box::pin(async move {
            tokio::time::sleep(COLLECT_DELAY).await;
            let args = serde_json::value::to_raw_value(&Collect::all())
                .map_err(|err| Error::Protocol(err.to_string()))?;
            let answer = self.0.call(Collect::NAME, &args, COLLECT_DELAY).await?;
            serde_json::from_str(answer.get())
                .map_err(|err| Error::Protocol(format!("unexpected answer to collect: {err}")))
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
        status::NodeState,
    };
    use uniwrt_node::{ControllerKey, Target};
    use uniwrt_sim::SimConfig;
    use uniwrt_store::{MasterKey, NodeRepo as _, Store, Vault};
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
        let fleet = Fleet::demo(&store, &config, clock.clone()).await.unwrap();
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

        jobs.shutdown().await;
        tokio::time::timeout(Duration::from_secs(5), fleet.stop())
            .await
            .unwrap();
        store.close().await;
    }
}

//! A scripted fleet for the apply job: each node answers at its addresses with the simulated
//! transaction tool (`uniwrt_sim::txn`), the agent's collect from a 24.10 container, and its
//! dnsmasq's answers; faults break it once a transaction is open.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::value::RawValue;
use uniwrt_core::{ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    job::{Job, JobState},
    node::{NewNode, Node, NodeMode, NodeRole},
    site::{
        SiteConfig,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::{
    ControllerKey, Credential, Error, ExecOutput, ExecRequest, ExitStatus, HostKey, Result, Target,
    proto::ops::{Collected, Set},
    txn::TOOL,
};
use uniwrt_sim::txn::{Now, Tool};
use uniwrt_store::{
    HostKeyRepo as _, JobRepo as _, MasterKey, NodeRepo as _, RevisionRepo as _, Store, Vault,
};

use super::radios::FakeRadios;
use crate::{
    ApplyTiming, BoxFuture, Dial, Jobs, JobsConfig, Remote, apply::plan::tests::collected,
    testing::Watched,
};

/// What breaks a node once a transaction is open on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// It answers no login.
    Unreachable,
    /// Its dnsmasq stops answering.
    DnsDown,
    /// Its timer runs out just before the confirm.
    Late,
}

pub(crate) type CollectHook = Box<dyn Fn(&Tool, &mut serde_json::Value) + Send>;
pub(crate) type ExecHook = Box<dyn Fn(&mut Tool, &[String]) -> Option<ExecOutput> + Send>;

pub(crate) struct FakeNode {
    pub(crate) host_key: HostKey,
    pub(crate) role: NodeRole,
    /// The addresses it answers at.
    pub(crate) hosts: Mutex<Vec<String>>,
    pub(crate) tool: Mutex<Tool>,
    pub(crate) uptime: AtomicU64,
    pub(crate) fault: Mutex<Option<Fault>>,
    /// The tool refuses this command (`check`, `apply`, …) with `invalid`.
    pub(crate) refuse: Mutex<Option<&'static str>>,
    pub(crate) down: AtomicBool,
    /// Rewrites what it collects from its `/etc/config` (the WAN test's ISP).
    pub(crate) collect_hook: Mutex<Option<CollectHook>>,
    /// Answers commands before the rest (the WAN test's `ping` and `wget`).
    pub(crate) exec_hook: Mutex<Option<ExecHook>>,
    /// The address the controller's host takes when the node's ports bounce (and loses when a
    /// rollback bounces them again); `None` for a host that keeps its address.
    pub(crate) renews_to: Mutex<Option<&'static str>>,
    /// The MTU its backhauls' VLAN devices take, which the agent's `mtu` op reports.
    pub(crate) mtu: Mutex<u32>,
    /// Its radios' channel switches and reloads (the agent's `ubus` op, `wifi reload`).
    pub(crate) radios: Mutex<FakeRadios>,
    /// The tool's commands run on every node, as `<node's first address> <command>`.
    journal: Arc<Mutex<Vec<String>>>,
}

impl FakeNode {
    fn new(role: NodeRole, host: &str, journal: Arc<Mutex<Vec<String>>>) -> Arc<Self> {
        let mut tool = Tool::stock();
        tool.footprint = Some(uniwrt_node::txn::Footprint::get().version().to_owned());
        Arc::new(Self {
            host_key: HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap())
                .unwrap(),
            role,
            hosts: Mutex::new(vec![host.to_owned()]),
            tool: Mutex::new(tool),
            uptime: AtomicU64::new(1000),
            fault: Mutex::default(),
            refuse: Mutex::default(),
            down: AtomicBool::new(false),
            collect_hook: Mutex::default(),
            exec_hook: Mutex::default(),
            renews_to: Mutex::default(),
            mtu: Mutex::new(1532),
            radios: Mutex::default(),
            journal,
        })
    }

    fn now(&self) -> Now {
        let uptime_s = self.uptime.load(Ordering::SeqCst);
        Now {
            uptime_s,
            unix_s: 1_800_000_000 + i64::try_from(uptime_s).unwrap(),
            booted_ms: 1,
        }
    }

    /// The fault, once a transaction is open.
    fn broken(&self) -> Option<Fault> {
        let open = self.tool.lock().unwrap().open().is_some();
        (*self.fault.lock().unwrap()).filter(|_| open)
    }

    pub(crate) fn file(&self, package: &str) -> Option<String> {
        self.tool.lock().unwrap().files.get(package).cloned()
    }

    fn run(&self, request: &ExecRequest) -> Result<ExecOutput> {
        if self.down.load(Ordering::SeqCst) || self.broken() == Some(Fault::Unreachable) {
            return Err(Error::Disconnected {
                what: "exec".to_owned(),
            });
        }
        let argv = &request.argv;
        if let Some(hook) = &*self.exec_hook.lock().unwrap()
            && let Some(output) = hook(&mut self.tool.lock().unwrap(), argv)
        {
            return Ok(output);
        }
        if argv[0] == "env" {
            // The controller logs in from 192.168.1.50, until its host asks for a lease again.
            let bounces = self.tool.lock().unwrap().bounced.len();
            let from = (*self.renews_to.lock().unwrap())
                .filter(|_| bounces % 2 == 1)
                .unwrap_or("192.168.1.50");
            return Ok(output(
                0,
                &format!("USER=root\nSSH_CONNECTION={from} 50022 192.168.1.1 22\n"),
            ));
        }
        if argv[0] == "wifi" {
            return Ok(self.radios.lock().unwrap().reload());
        }
        if argv[0] == "nslookup" {
            let up = self.role == NodeRole::Gateway && self.broken() != Some(Fault::DnsDown);
            return Ok(output(
                u32::from(!up),
                if up {
                    "Name:\tlocalhost\nAddress: 127.0.0.1\n"
                } else {
                    ""
                },
            ));
        }
        if argv[0] == TOOL {
            let command = argv[1].clone();
            let host = self.hosts.lock().unwrap()[0].clone();
            self.journal
                .lock()
                .unwrap()
                .push(format!("{host} {command}"));
            if *self.refuse.lock().unwrap() == Some(command.as_str()) {
                return Ok(output(
                    1,
                    r#"{"err":{"code":"invalid","msg":"uci: Parse error"}}"#,
                ));
            }
            if command == "confirm" && self.broken() == Some(Fault::Late) {
                self.uptime.fetch_add(3600, Ordering::SeqCst);
            }
        }
        let now = self.now();
        let mut tool = self.tool.lock().unwrap();
        Ok(tool
            .exec(argv, request.stdin.as_deref(), now)
            .unwrap_or_else(|| output(127, "")))
    }
}

pub(crate) fn output(code: u32, stdout: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

struct FleetDial(Vec<Arc<FakeNode>>);

impl Dial for FleetDial {
    fn banner<'a>(&'a self, _: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async { Ok(None) })
    }

    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(async move { Err(refused(target)) })
    }

    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        _credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        Box::pin(async move {
            let node = self
                .0
                .iter()
                .find(|node| node.hosts.lock().unwrap().contains(&target.host))
                .filter(|node| {
                    !node.down.load(Ordering::SeqCst) && node.broken() != Some(Fault::Unreachable)
                })
                .ok_or_else(|| refused(target))?;
            if *host_key != node.host_key {
                return Err(Error::HostKeyMismatch {
                    target: target.to_string(),
                    presented: node.host_key.clone(),
                    pinned: host_key.clone(),
                });
            }
            Ok(Box::new(FakeRemote(Arc::clone(node))) as Box<dyn Remote>)
        })
    }
}

fn refused(target: &Target) -> Error {
    Error::Connect {
        target: target.to_string(),
        source: std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"),
    }
}

struct FakeRemote(Arc<FakeNode>);

impl Remote for FakeRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move { self.0.run(&request) })
    }

    fn collect<'a>(&'a self, _sets: &'a [Set]) -> BoxFuture<'a, Result<Collected>> {
        Box::pin(async move {
            if self.0.down.load(Ordering::SeqCst) {
                return Err(Error::Disconnected {
                    what: "collect".to_owned(),
                });
            }
            let mut collected = collected(self.0.role);
            if let Some(hook) = &*self.0.collect_hook.lock().unwrap() {
                hook(&self.0.tool.lock().unwrap(), &mut collected);
            }
            self.0.radios.lock().unwrap().patch(&mut collected);
            Ok(serde_json::from_value(collected).unwrap())
        })
    }

    /// The agent's `mtu`: a VLAN device the node's `network` holds answers at its MTU, a peer
    /// replying when the size fits.
    fn call<'a>(
        &'a self,
        op: &'static str,
        args: &'a RawValue,
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<RawValue>>> {
        Box::pin(async move {
            if self.0.down.load(Ordering::SeqCst) || !["mtu", "ubus"].contains(&op) {
                return Err(Error::Disconnected {
                    what: op.to_owned(),
                });
            }
            let args: serde_json::Value = serde_json::from_str(args.get()).unwrap();
            if op == "ubus" {
                let answer = self.0.radios.lock().unwrap().ubus(&args)?;
                return Ok(serde_json::value::to_raw_value(&answer).unwrap());
            }
            let (dev, size) = (
                args["dev"].as_str().unwrap(),
                args["size"].as_u64().unwrap(),
            );
            let network = self.0.file("network").unwrap_or_default();
            if !network.contains(&format!("'{dev}'")) {
                return Err(Error::Remote {
                    op: op.to_owned(),
                    code: "not_found".to_owned(),
                    msg: format!("no device {dev}"),
                });
            }
            let mtu = *self.0.mtu.lock().unwrap();
            let peers: &[&str] = if u64::from(mtu) >= size {
                &["fe80::2"]
            } else {
                &[]
            };
            let answer = serde_json::json!({"dev": dev, "size": size, "mtu": mtu, "peers": peers});
            Ok(serde_json::value::to_raw_value(&answer).unwrap())
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

pub(crate) struct Fleet {
    _dir: tempfile::TempDir,
    pub(crate) store: Store,
    pub(crate) jobs: Jobs,
    pub(crate) watched: Arc<Watched>,
    /// The nodes in the store, and how each answers.
    pub(crate) nodes: Vec<(Node, Arc<FakeNode>)>,
    pub(crate) journal: Arc<Mutex<Vec<String>>>,
}

/// Adopted nodes with these roles and addresses, managed, and the site's first revision
/// (`super::super::plan::tests::site`) managing every package on them.
pub(crate) async fn fleet(members: &[(NodeRole, &str)]) -> Fleet {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_secs(1_793_059_200)));
    let store = Store::open(dir.path().join("test.db"), clock.clone())
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let mut nodes = Vec::new();
    let journal = Arc::default();
    for (i, &(role, host)) in members.iter().enumerate() {
        let fake = FakeNode::new(role, host, Arc::clone(&journal));
        let node = store
            .create_node(NewNode {
                name: format!(
                    "{}-{i}",
                    if role == NodeRole::Gateway {
                        "gw"
                    } else {
                        "ap"
                    }
                ),
                mac: uniwrt_core::MacAddr::new([2, 0, 0, 0, 1, u8::try_from(i).unwrap()]),
                board_name: "container".to_owned(),
                host: host.to_owned(),
                port: 22,
                role,
            })
            .await
            .unwrap();
        store
            .pin_host_key(node.id, fake.host_key.pin(Timestamp::from_unix_secs(0)))
            .await
            .unwrap();
        let node = store
            .set_node_mode(node.id, NodeMode::Managed)
            .await
            .unwrap()
            .unwrap();
        nodes.push((node, fake));
    }
    let ids: Vec<NodeId> = nodes.iter().map(|(node, _)| node.id).collect();
    commit(&store, None, super::super::plan::tests::site(&ids)).await;
    let watched = Arc::new(Watched::default());
    let dial = FleetDial(nodes.iter().map(|(_, fake)| Arc::clone(fake)).collect());
    let jobs = Jobs::start_with(
        JobsConfig {
            store: store.clone(),
            vault,
            clock,
            key: ControllerKey::generate().unwrap(),
            dial: Some(Arc::new(dial)),
            watch: Arc::clone(&watched) as _,
        },
        ApplyTiming {
            poll: Duration::from_millis(5),
            verify: Duration::from_millis(300),
            verify_uplink: Duration::from_millis(300),
            wan_wait: Duration::from_millis(300),
            ..ApplyTiming::default()
        },
    )
    .await
    .unwrap();
    Fleet {
        _dir: dir,
        store,
        jobs,
        watched,
        nodes,
        journal,
    }
}

/// Commits `config` as the next revision.
pub(crate) async fn commit(
    store: &Store,
    base: Option<uniwrt_core::RevisionId>,
    config: SiteConfig,
) -> uniwrt_core::RevisionId {
    store
        .commit_revision(
            NewRevision {
                base,
                kind: if base.is_some() {
                    RevisionKind::Edit
                } else {
                    RevisionKind::Defaults
                },
                restored_from: None,
                author_id: None,
                author: None,
                message: "test".to_owned(),
                config,
            },
            Vec::new(),
        )
        .await
        .unwrap()
        .id
}

impl Fleet {
    pub(crate) fn fake(&self, i: usize) -> &FakeNode {
        &self.nodes[i].1
    }

    /// The tool's commands so far, as `<address> <command>`, every `status` left out.
    pub(crate) fn journal(&self) -> Vec<String> {
        self.journal
            .lock()
            .unwrap()
            .iter()
            .filter(|line| !line.ends_with(" status"))
            .cloned()
            .collect()
    }

    pub(crate) async fn head(&self) -> uniwrt_model::site::revision::Revision {
        self.store.head_revision().await.unwrap().unwrap()
    }

    /// Applies the latest revision and waits for the job to end.
    pub(crate) async fn apply(&self) -> Job {
        let revision = self.head().await.meta.id;
        let mut updates = self.jobs.subscribe();
        let started = self.jobs.apply(revision, None).await.unwrap();
        let job = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let job = updates.recv().await.unwrap();
                if job.id == started.id && job.state != JobState::Running {
                    return job;
                }
            }
        })
        .await
        .expect("the apply ends");
        assert_eq!(self.store.job(job.id).await.unwrap().as_ref(), Some(&*job));
        (*job).clone()
    }
}

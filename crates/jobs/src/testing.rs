//! A scripted node for the jobs' tests: a [`Dial`] that plays dropbear, the agent (from a capture
//! of OpenWrt 24.10 in a container) and the transaction tool, and a temporary store.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Notify;
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_model::{
    facts::NodeFacts,
    job::{Job, JobState, StepName, StepState},
    node::Node,
};
use uniwrt_node::{
    ControllerKey, Credential, ExecOutput, ExecRequest, ExitStatus, HostKey, Result, Target,
    proto::ops::Collected,
    txn::{self, Footprint},
};
use uniwrt_store::{JobRepo as _, MasterKey, Store, Vault};
use zeroize::Zeroizing;

use crate::adopt::{AdoptRequest, INSTALL_KEY};
use crate::{BoxFuture, Dial, Jobs, JobsConfig, Remote, Watch};

pub(crate) const COLLECTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../node/fixtures/collect/x86-64-24.10.8.json"
));
pub(crate) const PASSWORD: &str = "correct horse battery staple";
pub(crate) const OPKG_STATUS: &str = "Package: dropbear\nVersion: 2024.86-r3\nStatus: install ok installed\n\
                           Package: luci\nVersion: 25.1\nStatus: install user installed\n";
pub(crate) const TARBALL: &[u8] = b"\x1f\x8b\x08\x00 etc/config/network";

/// A node as the job sees it over SSH.
pub(crate) struct FakeNode {
    pub(crate) host_key: HostKey,
    pub(crate) release: Mutex<String>,
    pub(crate) authorized_keys: Mutex<String>,
    /// Every command run, by argv.
    pub(crate) commands: Mutex<Vec<Vec<String>>>,
    /// Logins wait here while held.
    pub(crate) gate: Mutex<Option<Arc<Notify>>>,
    /// The version of the transaction tool installed, if any.
    pub(crate) footprint: Mutex<Option<String>>,
    /// The id of the transaction open on the node, if any.
    pub(crate) open_txn: Mutex<Option<String>>,
}

impl FakeNode {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            host_key: HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap())
                .unwrap(),
            release: Mutex::new("DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='24.10.8'\n".to_owned()),
            authorized_keys: Mutex::default(),
            commands: Mutex::default(),
            gate: Mutex::default(),
            footprint: Mutex::default(),
            open_txn: Mutex::default(),
        })
    }

    pub(crate) fn commands(&self) -> Vec<String> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .map(|argv| argv[0].clone())
            .collect()
    }

    fn run(&self, argv: &[String]) -> ExecOutput {
        self.commands.lock().unwrap().push(argv.to_vec());
        let ok = |stdout: &[u8]| ExecOutput {
            status: ExitStatus::Code(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        match argv.as_slice() {
            ["cat", "/etc/openwrt_release"] => ok(self.release.lock().unwrap().as_bytes()),
            ["sh", "-c", script, "sh", line] if *script == INSTALL_KEY => {
                let mut keys = self.authorized_keys.lock().unwrap();
                if keys.lines().any(|known| known == *line) {
                    ok(b"present\n")
                } else {
                    keys.push_str(line);
                    keys.push('\n');
                    ok(b"added\n")
                }
            }
            ["grep", _, _, "/usr/lib/opkg/status"] => ok(OPKG_STATUS.as_bytes()),
            ["sysupgrade", "-b", "-"] => ok(TARBALL),
            ["sh", "-c", script] if *script == script_of(&txn::status().request) => {
                let footprint = self.footprint.lock().unwrap();
                let open = self.open_txn.lock().unwrap().as_ref().map_or_else(
                    || "null".to_owned(),
                    |id| format!(r#"{{"id":"{id}","state":"applied","remaining_s":90}}"#),
                );
                ok(match &*footprint {
                    Some(version) => {
                        format!(r#"{{"ok":{{"version":"{version}","open":{open},"last":null}}}}"#)
                    }
                    None => r#"{"ok":null}"#.to_owned(),
                }
                .as_bytes())
            }
            ["sh", "-c", script] if *script == script_of(&Footprint::get().install().request) => {
                *self.footprint.lock().unwrap() = Some(Footprint::get().version().to_owned());
                ok(br#"{"ok":{}}"#)
            }
            ["sh", "-c", script] if *script == script_of(&txn::uninstall().request) => {
                if self.open_txn.lock().unwrap().is_some() {
                    return ExecOutput {
                        status: ExitStatus::Code(1),
                        stdout: br#"{"err":{"code":"busy","msg":"transaction j1 is open"}}"#
                            .to_vec(),
                        stderr: Vec::new(),
                    };
                }
                *self.footprint.lock().unwrap() = None;
                ok(br#"{"ok":{}}"#)
            }
            _ => ExecOutput {
                status: ExitStatus::Code(127),
                stdout: Vec::new(),
                stderr: b"not found".to_vec(),
            },
        }
    }
}

/// The script of an `sh -c` request.
fn script_of(request: &ExecRequest) -> String {
    request.argv[2].clone()
}

pub(crate) struct FakeDial(pub(crate) Arc<FakeNode>);

impl Dial for FakeDial {
    fn banner<'a>(&'a self, _target: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async { Ok(Some("SSH-2.0-dropbear_2024.86".to_owned())) })
    }

    fn probe<'a>(&'a self, _target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(async { Ok(self.0.host_key.clone()) })
    }

    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        Box::pin(async move {
            let gate = self.0.gate.lock().unwrap().clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
            if *host_key != self.0.host_key {
                return Err(uniwrt_node::Error::HostKeyMismatch {
                    target: target.to_string(),
                    presented: self.0.host_key.clone(),
                    pinned: host_key.clone(),
                });
            }
            let accepted = match credential {
                Credential::Password(password) => password.as_str() == PASSWORD,
                Credential::Key(key) => self
                    .0
                    .authorized_keys
                    .lock()
                    .unwrap()
                    .lines()
                    .any(|line| line == key.authorized_key().unwrap()),
            };
            if !accepted {
                return Err(uniwrt_node::Error::AuthRejected {
                    target: target.to_string(),
                    user: "root".to_owned(),
                    method: "password",
                });
            }
            Ok(Box::new(FakeRemote(Arc::clone(&self.0))) as Box<dyn Remote>)
        })
    }
}

pub(crate) struct FakeRemote(pub(crate) Arc<FakeNode>);

impl Remote for FakeRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move { Ok(self.0.run(&request.argv)) })
    }

    fn collect(&self) -> BoxFuture<'_, Result<Collected>> {
        Box::pin(async { Ok(serde_json::from_str(COLLECTED).unwrap()) })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[derive(Default)]
pub(crate) struct Watched(pub(crate) Mutex<Vec<(Node, HostKey, NodeFacts)>>);

impl Watch for Watched {
    fn watch(&self, node: Node, host_key: HostKey, facts: NodeFacts) -> BoxFuture<'_, ()> {
        self.0.lock().unwrap().push((node, host_key, facts));
        Box::pin(async {})
    }
}

pub(crate) struct Setup {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) store: Store,
    pub(crate) vault: Vault,
    pub(crate) key: ControllerKey,
    pub(crate) node: Arc<FakeNode>,
    pub(crate) watched: Arc<Watched>,
    pub(crate) jobs: Jobs,
}

pub(crate) async fn setup(dial: bool) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_secs(1_793_059_200)));
    let store = Store::open(dir.path().join("test.db"), clock.clone())
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let key = ControllerKey::generate().unwrap();
    let node = FakeNode::new();
    let watched = Arc::new(Watched::default());
    let jobs = Jobs::start(JobsConfig {
        store: store.clone(),
        vault: vault.clone(),
        clock,
        key: key.clone(),
        dial: dial.then(|| Arc::new(FakeDial(Arc::clone(&node))) as Arc<dyn Dial>),
        watch: Arc::clone(&watched) as Arc<dyn Watch>,
    })
    .await
    .unwrap();
    Setup {
        _dir: dir,
        store,
        vault,
        key,
        node,
        watched,
        jobs,
    }
}

impl Setup {
    pub(crate) fn request(&self, password: Option<&str>) -> AdoptRequest {
        AdoptRequest {
            target: Target::new("10.0.0.2", 22),
            host_key: self.node.host_key.clone(),
            password: password.map(|password| Zeroizing::new(password.to_owned())),
            remember_password: false,
            name: None,
            role: None,
        }
    }

    /// Adopts and waits for the job to end; returns the updates announced, the last one first.
    pub(crate) async fn adopt(&self, request: AdoptRequest) -> (Job, Vec<Arc<Job>>) {
        let mut updates = self.jobs.subscribe();
        let started = self.jobs.adopt(request, None).await.unwrap();
        assert_eq!(started.state, JobState::Running);
        let mut seen = Vec::new();
        let job = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let job = updates.recv().await.unwrap();
                seen.push(Arc::clone(&job));
                if job.id == started.id && job.state != JobState::Running {
                    return job;
                }
            }
        })
        .await
        .expect("the job ends");
        assert_eq!(self.store.job(job.id).await.unwrap().as_ref(), Some(&*job));
        ((*job).clone(), seen)
    }
}

pub(crate) fn states(job: &Job) -> Vec<(StepName, StepState)> {
    job.steps
        .iter()
        .map(|step| (step.name, step.state))
        .collect()
}

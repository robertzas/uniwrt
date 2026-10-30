//! Adoptions over a scripted node (a [`Dial`] that plays dropbear and the agent from a capture of
//! OpenWrt 24.10 in a container) and a temporary store.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::{Notify, broadcast};
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_model::{
    facts::NodeFacts,
    job::{Job, JobErrorCode, JobState, StepName, StepState},
    node::{Node, NodeMode, NodeRole},
};
use uniwrt_node::{
    ControllerKey, Credential, ExecOutput, ExecRequest, ExitStatus, HostKey, Result, Target,
    proto::ops::Collected,
};
use uniwrt_store::{
    HostKeyRepo as _, InventoryRepo as _, JobRepo as _, MasterKey, NodeRepo as _, Store, Vault,
    VaultRepo as _,
};
use zeroize::Zeroizing;

use super::{AdoptRequest, INSTALL_KEY};
use crate::{BoxFuture, Dial, Error, Jobs, JobsConfig, Remote, Watch};

const COLLECTED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../node/fixtures/collect/x86-64-24.10.8.json"
));
const PASSWORD: &str = "correct horse battery staple";
const OPKG_STATUS: &str = "Package: dropbear\nVersion: 2024.86-r3\nStatus: install ok installed\n\
                           Package: luci\nVersion: 25.1\nStatus: install user installed\n";
const TARBALL: &[u8] = b"\x1f\x8b\x08\x00 etc/config/network";

/// A node as the job sees it over SSH.
struct FakeNode {
    host_key: HostKey,
    release: Mutex<String>,
    authorized_keys: Mutex<String>,
    /// Every command run, by argv.
    commands: Mutex<Vec<Vec<String>>>,
    /// Logins wait here while held.
    gate: Mutex<Option<Arc<Notify>>>,
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
        })
    }

    fn commands(&self) -> Vec<String> {
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
            _ => ExecOutput {
                status: ExitStatus::Code(127),
                stdout: Vec::new(),
                stderr: b"not found".to_vec(),
            },
        }
    }
}

struct FakeDial(Arc<FakeNode>);

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

struct FakeRemote(Arc<FakeNode>);

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
struct Watched(Mutex<Vec<(Node, HostKey, NodeFacts)>>);

impl Watch for Watched {
    fn watch(&self, node: Node, host_key: HostKey, facts: NodeFacts) -> BoxFuture<'_, ()> {
        self.0.lock().unwrap().push((node, host_key, facts));
        Box::pin(async {})
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    store: Store,
    vault: Vault,
    key: ControllerKey,
    node: Arc<FakeNode>,
    watched: Arc<Watched>,
    jobs: Jobs,
}

async fn setup(dial: bool) -> Setup {
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
    fn request(&self, password: Option<&str>) -> AdoptRequest {
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
    async fn adopt(&self, request: AdoptRequest) -> (Job, Vec<Arc<Job>>) {
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

fn states(job: &Job) -> Vec<(StepName, StepState)> {
    job.steps
        .iter()
        .map(|step| (step.name, step.state))
        .collect()
}

#[tokio::test]
async fn adopting_with_the_password_installs_the_key_records_everything_and_watches() {
    let setup = setup(true).await;
    let mut request = setup.request(Some(PASSWORD));
    request.remember_password = true;
    let (job, seen) = setup.adopt(request).await;

    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert!(job.steps.iter().all(|step| step.state == StepState::Done));
    assert!(job.finished_at.is_some());
    assert!(
        seen.len() > 2 * job.steps.len(),
        "every step's start and end"
    );
    let id = job.node.unwrap();

    // Only the controller's key changed on the node.
    assert_eq!(
        *setup.node.authorized_keys.lock().unwrap(),
        format!("{}\n", setup.key.authorized_key().unwrap())
    );
    assert_eq!(
        setup.node.commands(),
        ["cat", "sh", "grep", "sysupgrade"],
        "read the release, install the key, read the packages, back up"
    );

    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mode, NodeMode::Observe);
    assert_eq!(node.role, NodeRole::Ap);
    assert_eq!((node.host.as_str(), node.port), ("10.0.0.2", 22));
    assert_eq!(node.name, "OpenWrt");
    assert_eq!(node.board_name, "container");
    let pin = setup.store.host_key(id).await.unwrap().unwrap();
    assert_eq!(pin.fingerprint, setup.node.host_key.fingerprint());
    let inventory = setup.store.inventory(id).await.unwrap().unwrap();
    let names: Vec<_> = inventory
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package.explicit))
        .collect();
    assert_eq!(names, [("dropbear", false), ("luci", true)]);
    let backups = setup.store.backups(id).await.unwrap();
    let (_, tarball) = setup
        .vault
        .open_backup(backups[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tarball.as_slice(), TARBALL);
    let password = setup
        .vault
        .secret(&format!("node/{id}/root_password"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(password.as_slice(), PASSWORD.as_bytes());

    let watched = setup.watched.0.lock().unwrap();
    assert_eq!(watched.len(), 1);
    assert_eq!(watched[0].0, node);
    assert_eq!(watched[0].1, setup.node.host_key);
    assert_eq!(watched[0].2.board.board_name, "container");
}

#[tokio::test]
async fn a_node_that_trusts_the_key_needs_no_password_and_no_change() {
    let setup = setup(true).await;
    setup
        .node
        .authorized_keys
        .lock()
        .unwrap()
        .push_str(&(setup.key.authorized_key().unwrap() + "\n"));
    let mut request = setup.request(None);
    request.name = Some("  attic  ".to_owned());
    request.role = Some(NodeRole::Bridge);
    let (job, _) = setup.adopt(request).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let skipped: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.state == StepState::Skipped)
        .map(|step| step.name)
        .collect();
    assert_eq!(skipped, [StepName::InstallKey, StepName::VerifyKey]);
    assert!(!setup.node.commands().contains(&"sh".to_owned()));
    let node = setup.store.node(job.node.unwrap()).await.unwrap().unwrap();
    assert_eq!((node.name.as_str(), node.role), ("attic", NodeRole::Bridge));
    assert!(
        setup
            .vault
            .secret(&format!("node/{}/root_password", node.id))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_wrong_password_fails_at_connect_and_changes_nothing() {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some("wrong"))).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.as_ref().unwrap().code, JobErrorCode::AuthRejected);
    assert_eq!(
        states(&job)[..2],
        [
            (StepName::Connect, StepState::Failed),
            (StepName::Check, StepState::Pending)
        ]
    );
    assert_eq!(setup.node.commands(), [""; 0]);
    assert_eq!(setup.store.nodes().await.unwrap(), []);
    assert!(setup.watched.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn another_host_key_than_the_confirmed_one_is_refused() {
    let setup = setup(true).await;
    let mut request = setup.request(Some(PASSWORD));
    request.host_key =
        HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap()).unwrap();
    let (job, _) = setup.adopt(request).await;
    assert_eq!(job.error.unwrap().code, JobErrorCode::HostKeyMismatch);
    assert_eq!(setup.node.commands(), [""; 0]);
}

#[tokio::test]
async fn an_old_release_fails_the_check_before_any_change() {
    let setup = setup(true).await;
    *setup.node.release.lock().unwrap() =
        "DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='23.05.5'\n".to_owned();
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::Unsupported);
    assert!(error.message.contains("23.05.5"), "{error:?}");
    assert_eq!(states(&job)[1], (StepName::Check, StepState::Failed));
    assert_eq!(setup.node.commands(), ["cat"]);
    assert!(setup.node.authorized_keys.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_adopted_already_fails_the_check() {
    let setup = setup(true).await;
    let (first, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(first.state, JobState::Succeeded);
    let (again, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let error = again.error.unwrap();
    assert_eq!(error.code, JobErrorCode::AlreadyAdopted);
    assert_eq!(error.message, "The node is adopted already, as OpenWrt");
    assert_eq!(setup.store.nodes().await.unwrap().len(), 1);
    assert_eq!(setup.jobs.jobs(10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn one_job_at_a_time_per_target_and_a_stop_interrupts_it() {
    let setup = setup(true).await;
    let gate = Arc::new(Notify::new());
    *setup.node.gate.lock().unwrap() = Some(Arc::clone(&gate));
    let mut updates = setup.jobs.subscribe();
    let job = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap();
    // The job is connecting, held at the gate.
    wait_for(&mut updates, |job| job.steps[0].state == StepState::Running).await;
    let err = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Busy { .. }), "{err:?}");

    setup.jobs.shutdown().await;
    let stopped = setup.store.job(job.id).await.unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Failed);
    assert_eq!(stopped.error.unwrap().code, JobErrorCode::Interrupted);
    assert_eq!(stopped.steps[0].state, StepState::Failed);
    assert_eq!(setup.store.nodes().await.unwrap(), []);
}

#[tokio::test]
async fn a_finished_job_frees_its_target() {
    let setup = setup(true).await;
    let (first, _) = setup.adopt(setup.request(Some("wrong"))).await;
    assert_eq!(first.state, JobState::Failed);
    let (second, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(second.state, JobState::Succeeded);
}

#[tokio::test]
async fn without_a_dial_nothing_can_be_adopted() {
    let setup = setup(false).await;
    assert!(!setup.jobs.can_adopt());
    let err = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unavailable));
    let target = Target::new("10.0.0.2", 22);
    assert!(matches!(
        setup.jobs.probe(&target).await.unwrap_err(),
        Error::Unavailable
    ));
    assert_eq!(setup.jobs.jobs(10).await.unwrap(), []);
}

async fn wait_for(updates: &mut broadcast::Receiver<Arc<Job>>, done: impl Fn(&Job) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done(&updates.recv().await.unwrap()) {}
    })
    .await
    .expect("the update comes");
}

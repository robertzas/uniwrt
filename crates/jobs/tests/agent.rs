//! Adopting real OpenWrt (TESTING.md, Agent): the adopt job over SSH against a container booted
//! with procd, at `UNIWRT_NODE=<host:port>` with the root password in `UNIWRT_NODE_PASSWORD`
//! (`just test-agent`). Checks that adoption changes nothing on the node but `authorized_keys`.

#![allow(clippy::unwrap_used)]

use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::broadcast;
use uniwrt_core::{JobId, SystemClock};
use uniwrt_jobs::{AdoptRequest, BoxFuture, Jobs, JobsConfig, SshDial, Watch};
use uniwrt_model::{
    facts::NodeFacts,
    job::{Job, JobErrorCode, JobState, StepState},
    node::{Node, NodeMode},
};
use uniwrt_node::{
    ControllerKey, Credential, ExecRequest, HostKey, Session, SshConfig, Target, probe,
};
use uniwrt_store::{InventoryRepo as _, MasterKey, Store, Vault};
use zeroize::Zeroizing;

#[derive(Default)]
struct Watched(Mutex<Vec<Node>>);

impl Watch for Watched {
    fn watch(&self, node: Node, _host_key: HostKey, _facts: NodeFacts) -> BoxFuture<'_, ()> {
        self.0.lock().unwrap().push(node);
        Box::pin(async {})
    }
}

async fn run(session: &Session, argv: &[&str]) -> String {
    let output = session
        .exec(&ExecRequest::new(argv.iter().copied()))
        .await
        .unwrap()
        .check(argv[0])
        .unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// Waits for the job to end, at most a minute.
async fn ended(updates: &mut broadcast::Receiver<Arc<Job>>, id: JobId) -> Arc<Job> {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let job = updates.recv().await.unwrap();
            if job.id == id && job.state != JobState::Running {
                return job;
            }
        }
    })
    .await
    .expect("the job ends")
}

/// The paths changed since the mark, outside the virtual file systems and /tmp.
async fn changed_since_mark(session: &Session) -> String {
    let pruned = ["/proc", "/sys", "/dev", "/tmp"];
    let mut argv = vec!["find", "/", "-xdev", "("];
    for (i, path) in pruned.iter().enumerate() {
        if i > 0 {
            argv.push("-o");
        }
        argv.extend(["-path", path]);
    }
    argv.extend([
        ")",
        "-prune",
        "-o",
        "-newer",
        "/tmp/uniwrt-adoption-mark",
        "-print",
    ]);
    run(session, &argv).await
}

#[tokio::test]
#[ignore = "needs OpenWrt at UNIWRT_NODE; run by `just test-agent`"]
async fn adoption_changes_only_authorized_keys() {
    let address = env::var("UNIWRT_NODE").expect("UNIWRT_NODE=<host:port>");
    let password = env::var("UNIWRT_NODE_PASSWORD").expect("UNIWRT_NODE_PASSWORD");
    let (host, port) = address.rsplit_once(':').unwrap();
    let target = Target::new(host, port.parse().unwrap());

    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("test.db"), clock.clone())
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let key = ControllerKey::generate().unwrap();
    let watched = Arc::new(Watched::default());
    let jobs = Jobs::start(JobsConfig {
        store: store.clone(),
        vault: vault.clone(),
        clock,
        key: key.clone(),
        dial: Some(Arc::new(SshDial::default())),
        watch: Arc::clone(&watched) as Arc<dyn Watch>,
    })
    .await
    .unwrap();

    // What the user confirms, and a mark to find what changes after it.
    let host_key = jobs.probe(&target).await.unwrap();
    assert_eq!(
        host_key,
        probe(&target, None, &SshConfig::default()).await.unwrap()
    );
    let root = Credential::Password(Zeroizing::new(password.clone()));
    let session = Session::connect(&target, None, &host_key, &root, &SshConfig::default())
        .await
        .unwrap();
    run(&session, &["touch", "/tmp/uniwrt-adoption-mark"]).await;
    // Modification times have one-second resolution on some file systems.
    tokio::time::sleep(Duration::from_millis(1100)).await;

    let request = |password: Option<&str>| AdoptRequest {
        target: target.clone(),
        host_key: host_key.clone(),
        password: password.map(|password| Zeroizing::new(password.to_owned())),
        remember_password: false,
        name: None,
        role: None,
    };
    let mut updates = jobs.subscribe();
    let started = jobs.adopt(request(Some(&password)), None).await.unwrap();
    let job = ended(&mut updates, started.id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert!(job.steps.iter().all(|step| step.state == StepState::Done));

    let node = watched.0.lock().unwrap()[0].clone();
    assert_eq!(node.mode, NodeMode::Observe);
    assert_eq!(node.port, target.port);
    let inventory = store.inventory(node.id).await.unwrap().unwrap();
    assert!(
        inventory
            .packages
            .iter()
            .any(|package| package.name == "dropbear" && !package.version.is_empty()),
        "{inventory:?}"
    );
    let backups = store.backups(node.id).await.unwrap();
    let (_, tarball) = vault.open_backup(backups[0].id).await.unwrap().unwrap();
    assert!(tarball.starts_with(&[0x1f, 0x8b]));
    assert!(tarball.len() > 1024);

    // The node trusts the key now, and nothing else changed outside /tmp.
    let with_key = Session::connect(
        &target,
        None,
        &host_key,
        &Credential::Key(key),
        &SshConfig::default(),
    )
    .await
    .unwrap();
    let changed = changed_since_mark(&with_key).await;
    assert!(
        changed
            .lines()
            .any(|path| path == "/etc/dropbear/authorized_keys"),
        "{changed}"
    );
    // Other tests in the same container install their keys meanwhile, which changes the same
    // two paths.
    for path in changed.lines() {
        assert!(
            ["/etc/dropbear", "/etc/dropbear/authorized_keys"].contains(&path),
            "adoption changed {path}"
        );
    }

    // Adopting it again is refused at the check, before any change.
    let again = jobs.adopt(request(None), None).await.unwrap();
    let job = ended(&mut updates, again.id).await;
    assert_eq!(
        job.error.as_ref().unwrap().code,
        JobErrorCode::AlreadyAdopted
    );
    session.close().await;
    with_key.close().await;
    store.close().await;
}

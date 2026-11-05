//! End-to-end applies (TESTING.md, System; D-070) on scripts/test-system.sh's topology,
//! `isp ═ gw ═ ap`: the controller's apply job reaches the gateway and the AP over a
//! `docker exec` dial ([`dial`]), turns management on (installing the transaction tool and the
//! guard), and applies revisions through the real tool, agent and health checks: a confirmed
//! apply, a broken WAN that rolls both back, and a lost AP that rolls back at its deadline.
//!
//! Each test starts from the same ground: no open transaction, the defaults behind DHCP written
//! directly (`Node::apply`), the nodes adopted into a new store with that site as revision 1.
//! The timers are the tool's shortest, so a rollback at a deadline takes seconds, not minutes.

mod dial;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use uniwrt_core::{MacAddr, NodeId, SystemClock, Timestamp};
use uniwrt_jobs::{ApplyTiming, BoxFuture, Jobs, JobsConfig, Watch};
use uniwrt_model::{
    apply::PlanAction,
    facts::NodeFacts,
    job::{Job, JobErrorCode, JobNodeState, JobState},
    node::{NewNode, Node as NodeRecord, NodeRole},
    site::{
        SiteConfig,
        node::{RawSection, RawValue},
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::{ControllerKey, HostKey, txn};
use uniwrt_store::{
    ApplyRepo as _, HostKeyRepo as _, MasterKey, NodeRepo as _, RevisionRepo as _, Store, Vault,
};
use uniwrt_system::{
    Node, has_ipv4, ipv4, render_for,
    site::{self, AP, GATEWAY, ISP_NET, Uplink},
};

use self::dial::DockerDial;

const GW: &str = "uniwrt-system-gw";
const AP_CONTAINER: &str = "uniwrt-system-ap";
const UP: Duration = Duration::from_secs(45);

/// The tool's shortest timer (`txn::MIN_TIMEOUT`, plus margin) and verify windows within it.
fn timing() -> ApplyTiming {
    ApplyTiming {
        poll: Duration::from_secs(1),
        verify: Duration::from_secs(25),
        verify_uplink: Duration::from_secs(45),
        timer: Duration::from_secs(40),
        timer_uplink: Duration::from_secs(60),
    }
}

/// The apply moves no node in these tests; watching is the engine's, which isn't here.
struct NoWatch;

impl Watch for NoWatch {
    fn watch(&self, _: NodeRecord, _: HostKey, _: NodeFacts) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }

    fn moved(&self, node: NodeRecord, _: HostKey) -> BoxFuture<'_, ()> {
        panic!(
            "{} moved to {}, which no test renumbers",
            node.name, node.host
        )
    }
}

struct Lab {
    _dir: tempfile::TempDir,
    store: Store,
    jobs: Jobs,
    dial: DockerDial,
    gw: Node,
    ap: Node,
    base: SiteConfig,
}

/// Waits until no transaction is open on `container`, rolling back one a failed run left.
async fn settle(container: &str) {
    let status = txn::status();
    let output = dial::exec(container, &status.request)
        .await
        .expect("docker exec");
    let Some(status) = status.parse(output).expect("the tool's status") else {
        return; // the tool isn't installed
    };
    if let Some(open) = status.open
        && open.state != txn::OpenState::RollingBack
    {
        let id = txn::TxnId::new(open.id).expect("an id");
        let _ = dial::exec(container, &txn::rollback(&id).request).await;
    }
    wait_status(container, Duration::from_secs(90), |s| s.open.is_none()).await;
}

/// Waits until the tool's status on `container` satisfies `ready`, and returns it.
async fn wait_status(
    container: &str,
    timeout: Duration,
    ready: impl Fn(&txn::Status) -> bool,
) -> txn::Status {
    let until = tokio::time::Instant::now() + timeout;
    loop {
        let status = txn::status();
        let output = dial::exec(container, &status.request)
            .await
            .expect("docker exec");
        let status = status
            .parse(output)
            .expect("the tool's status")
            .expect("installed");
        if ready(&status) {
            return status;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "{container}: the tool's status stayed {status:#?}"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn in_subnet(status: &serde_json::Value, prefix: &str) -> bool {
    has_ipv4(status) && ipv4(status).iter().any(|a| a.starts_with(prefix))
}

/// The gateway's WAN holds a lease from the ISP, with a default route.
fn wan_up(status: &serde_json::Value) -> bool {
    in_subnet(status, ISP_NET)
        && status["route"]
            .as_array()
            .is_some_and(|routes| routes.iter().any(|r| r["target"] == "0.0.0.0"))
}

fn hostname(node: &Node) -> String {
    node.sh("uci -q get system.@system[0].hostname", &[])
        .trim()
        .to_owned()
}

/// Writes the defaults behind DHCP onto both nodes directly; returns the AP's lease.
fn reset(base: &SiteConfig, gw: &Node, ap: &Node) -> String {
    gw.apply(&render_for(base, gw, GATEWAY, NodeRole::Gateway));
    gw.wait_interface("wan", UP, wan_up);
    gw.wait_interface("lan", UP, |s| ipv4(s) == ["10.0.0.1/24"]);
    ap.apply(&render_for(base, ap, AP, NodeRole::Ap));
    let lan = ap.wait_interface("lan", UP, |s| in_subnet(s, "10.0.0."));
    let address = ipv4(&lan)[0].clone();
    address.split('/').next().unwrap_or_default().to_owned()
}

async fn lab() -> Lab {
    settle(GW).await;
    settle(AP_CONTAINER).await;
    let (gw, ap) = (Node::new(GW), Node::new(AP_CONTAINER));
    let base = site::defaults(Uplink::Dhcp);
    let ap_host = tokio::task::block_in_place(|| reset(&base, &gw, &ap));

    let dir = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(SystemClock))
        .await
        .expect("the store opens");
    let vault = Vault::open(&store, &MasterKey::generate().expect("a key"))
        .await
        .expect("the vault opens");
    let dial = DockerDial::new(&[GW, AP_CONTAINER]).await;
    for (i, (name, container, host, role, id)) in [
        ("gw", GW, "10.0.0.1".to_owned(), NodeRole::Gateway, GATEWAY),
        ("ap", AP_CONTAINER, ap_host, NodeRole::Ap, AP),
    ]
    .into_iter()
    .enumerate()
    {
        let node = store
            .create_node(NewNode {
                name: name.to_owned(),
                mac: MacAddr::new([2, 0, 0, 0, 2, u8::try_from(i).expect("few")]),
                board_name: "container".to_owned(),
                host,
                port: 22,
                role,
            })
            .await
            .expect("the node is recorded");
        assert_eq!(node.id, id, "the site's ids are the store's first");
        let pin = dial.host_key(container).pin(Timestamp::from_unix_secs(0));
        store.pin_host_key(id, pin).await.expect("the pin");
    }
    commit(&store, None, base.clone()).await;
    let jobs = Jobs::start_with(
        JobsConfig {
            store: store.clone(),
            vault,
            clock: Arc::new(SystemClock),
            key: ControllerKey::generate().expect("a key"),
            dial: Some(Arc::new(dial.clone())),
            watch: Arc::new(NoWatch),
        },
        timing(),
    )
    .await
    .expect("the jobs start");
    let lab = Lab {
        _dir: dir,
        store,
        jobs,
        dial,
        gw,
        ap,
        base,
    };
    for id in [GATEWAY, AP] {
        let started = lab
            .jobs
            .manage(id, true, None)
            .await
            .expect("manage starts");
        let job = lab.wait(started).await;
        assert_eq!(job.state, JobState::Succeeded, "management on: {job:#?}");
    }
    lab
}

async fn commit(
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
                message: "system test".to_owned(),
                config,
            },
            Vec::new(),
        )
        .await
        .expect("the revision commits")
        .id
}

impl Lab {
    /// Waits for `started` to end.
    async fn wait(&self, started: Job) -> Job {
        let mut updates = self.jobs.subscribe();
        if let Some(job) = self.jobs.job(started.id).await.expect("the job")
            && job.state != JobState::Running
        {
            return job;
        }
        tokio::time::timeout(Duration::from_secs(240), async {
            loop {
                let job = updates.recv().await.expect("updates");
                if job.id == started.id && job.state != JobState::Running {
                    return (*job).clone();
                }
            }
        })
        .await
        .expect("the job ends")
    }

    /// The base site with new hostnames, and `edit`, committed as the next revision.
    async fn revise(&self, edit: impl FnOnce(&mut SiteConfig)) -> uniwrt_core::RevisionId {
        let mut config = self.base.clone();
        for (node, name) in config.nodes.iter_mut().zip(["e2e-gw", "e2e-ap"]) {
            node.hostname = Some(name.to_owned());
        }
        edit(&mut config);
        let head = self
            .store
            .head_revision()
            .await
            .expect("head")
            .expect("one");
        commit(&self.store, Some(head.meta.id), config).await
    }

    async fn apply(&self, revision: uniwrt_core::RevisionId) -> Job {
        let started = self
            .jobs
            .apply(revision, None)
            .await
            .expect("the apply starts");
        self.wait(started).await
    }

    fn state(job: &Job, id: NodeId) -> (JobNodeState, String) {
        let node = job.nodes.iter().find(|n| n.node == id).expect("in the job");
        (node.state, node.detail.clone().unwrap_or_default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs scripts/test-system.sh's topology"]
async fn a_confirmed_apply_holds_on_both() {
    let lab = lab().await;
    let before = (hostname(&lab.gw), hostname(&lab.ap));
    // The base revision is what the nodes hold: nothing to do.
    let plan = lab.jobs.plan().await.expect("a plan");
    assert!(
        plan.nodes.iter().all(|n| n.action == PlanAction::Unchanged),
        "{plan:#?}"
    );

    let revision = lab
        .revise(|config| {
            let lan = &mut config.networks[0];
            if let Some(dhcp) = lan.ipv4.as_mut().and_then(|ipv4| ipv4.dhcp.as_mut()) {
                dhcp.lease_time_s = 7200;
            }
        })
        .await;
    let plan = lab.jobs.plan().await.expect("a plan");
    assert!(plan.ready(), "{plan:#?}");
    for node in &plan.nodes {
        assert_eq!(node.action, PlanAction::Change, "{node:#?}");
        assert_eq!(node.timeout_s, 40, "{node:#?}");
    }

    let job = lab.apply(revision).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    for id in [GATEWAY, AP] {
        assert_eq!(Lab::state(&job, id).0, JobNodeState::Confirmed, "{job:#?}");
    }
    assert_ne!(before, (hostname(&lab.gw), hostname(&lab.ap)));
    assert_eq!(
        (hostname(&lab.gw), hostname(&lab.ap)),
        ("e2e-gw".to_owned(), "e2e-ap".to_owned())
    );
    assert_eq!(lab.gw.sh("uci -q get dhcp.lan.leasetime", &[]).trim(), "2h");
    let name = format!("uniwrt-{}", job.id);
    for container in [GW, AP_CONTAINER] {
        let status = wait_status(container, Duration::ZERO, |_| true).await;
        let last = status.last.expect("a finished transaction");
        assert_eq!(
            (status.open, last.id.as_str(), last.outcome),
            (None, name.as_str(), txn::Outcome::Confirmed),
            "{container}"
        );
    }
    lab.gw.wait_interface("wan", UP, wan_up);
    let applied: BTreeMap<NodeId, _> = lab
        .store
        .applied_revisions()
        .await
        .expect("applied revisions")
        .into_iter()
        .map(|a| (a.node, a.revision))
        .collect();
    assert_eq!(
        applied,
        BTreeMap::from([(GATEWAY, revision), (AP, revision)])
    );
    // What the nodes hold now renders the same: applying again would change nothing.
    let plan = lab.jobs.plan().await.expect("a plan");
    assert!(
        plan.nodes.iter().all(|n| n.action == PlanAction::Unchanged),
        "{plan:#?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs scripts/test-system.sh's topology"]
async fn a_broken_wan_rolls_both_back() {
    let lab = lab().await;
    let before = (hostname(&lab.gw), hostname(&lab.ap));
    let device = lab
        .gw
        .sh("uci -q get network.wan.device", &[])
        .trim()
        .to_owned();
    // A static WAN without its gateway: up, with no default route.
    let revision = lab
        .revise(|config| {
            let options = [
                ("proto", "static"),
                ("device", device.as_str()),
                ("ipaddr", "192.0.2.2"),
                ("netmask", "255.255.255.0"),
            ];
            config.nodes[0].raw.push(RawSection {
                package: "network".to_owned(),
                kind: "interface".to_owned(),
                name: Some("wan".to_owned()),
                options: options
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), RawValue::Option(v.to_owned())))
                    .collect(),
                merge: false,
            });
        })
        .await;
    let plan = lab.jobs.plan().await.expect("a plan");
    let gw_plan = plan
        .nodes
        .iter()
        .find(|n| n.node == GATEWAY)
        .expect("planned");
    assert_eq!(gw_plan.timeout_s, 60, "the WAN changes: {gw_plan:#?}");

    let job = lab.apply(revision).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.clone().expect("why");
    assert_eq!(error.code, JobErrorCode::RolledBack, "{job:#?}");
    assert!(error.message.contains("the WAN is down"), "{job:#?}");
    for id in [GATEWAY, AP] {
        assert_eq!(
            Lab::state(&job, id),
            (JobNodeState::RolledBack, "Rolled back".to_owned()),
            "{job:#?}"
        );
    }
    for container in [GW, AP_CONTAINER] {
        let status = wait_status(container, Duration::ZERO, |_| true).await;
        let last = status.last.expect("a finished transaction");
        assert_eq!(
            (status.open, last.outcome, last.reason),
            (None, txn::Outcome::RolledBack, Some(txn::Reason::Requested)),
            "{container}"
        );
    }
    assert_eq!(before, (hostname(&lab.gw), hostname(&lab.ap)));
    assert_eq!(
        lab.gw.sh("uci -q get network.wan.proto", &[]).trim(),
        "dhcp"
    );
    lab.gw.wait_interface("wan", UP, wan_up);
    assert_eq!(
        lab.store.applied_revisions().await.expect("applied"),
        Vec::new()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs scripts/test-system.sh's topology"]
async fn a_lost_node_rolls_back_at_its_deadline() {
    let lab = lab().await;
    let before = (hostname(&lab.gw), hostname(&lab.ap));
    let revision = lab.revise(|_| {}).await;
    lab.dial.lose_after_apply(AP_CONTAINER);

    let job = lab.apply(revision).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    assert_eq!(
        job.error.as_ref().map(|e| e.code),
        Some(JobErrorCode::RolledBack),
        "{job:#?}"
    );
    assert_eq!(
        Lab::state(&job, GATEWAY),
        (JobNodeState::RolledBack, "Rolled back".to_owned()),
        "{job:#?}"
    );
    let (state, detail) = Lab::state(&job, AP);
    assert_eq!(state, JobNodeState::RolledBack, "{job:#?}");
    assert!(
        detail.starts_with("Unreachable: it rolls back by itself"),
        "{job:#?}"
    );
    // The gateway is back at once; the AP once its own timer runs out.
    assert_eq!(hostname(&lab.gw), before.0);
    let status = wait_status(AP_CONTAINER, timing().timer, |s| s.open.is_none()).await;
    let last = status.last.expect("a finished transaction");
    assert_eq!(
        (last.id, last.outcome, last.reason),
        (
            format!("uniwrt-{}", job.id),
            txn::Outcome::RolledBack,
            Some(txn::Reason::Timeout)
        )
    );
    assert_eq!(hostname(&lab.ap), before.1);
}

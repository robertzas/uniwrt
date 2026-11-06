//! Applies over the scripted fleet: the two-phase commit, its failures and rollbacks.

use std::sync::atomic::Ordering;

use uniwrt_model::{
    apply::PlanAction,
    job::{Job, JobErrorCode, JobKind, JobNodeState, JobState, StepName, StepState},
    node::{NodeMode, NodeRole},
};
use uniwrt_node::txn::{self, Outcome, Reason};
use uniwrt_sim::txn::Now;
use uniwrt_store::{ApplyRepo as _, DriftRepo as _, NodeRepo as _};

use super::fleet::{Fault, Fleet, commit, fleet};
use crate::{Error, testing::states};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP1: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");
const AP2: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.22");

fn node_states(job: &Job) -> Vec<JobNodeState> {
    job.nodes.iter().map(|node| node.state).collect()
}

fn failed_with(job: &Job, code: JobErrorCode) -> String {
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, code, "{}", error.message);
    error.message
}

/// What the node's tool reports of its last transaction.
fn last(fleet: &Fleet, i: usize) -> (Outcome, Option<Reason>) {
    let fake = fleet.fake(i);
    let request = txn::status();
    let now = Now {
        uptime_s: fake.uptime.load(Ordering::SeqCst),
        unix_s: 0,
        booted_ms: 1,
    };
    let output = fake
        .tool
        .lock()
        .unwrap()
        .exec(&request.request.argv, None, now)
        .unwrap();
    let last = request.parse(output).unwrap().unwrap().last.unwrap();
    (last.outcome, last.reason)
}

#[tokio::test]
async fn an_apply_stages_everywhere_then_applies_leaves_first_and_confirms_all() {
    let fleet = fleet(&[GW, AP1, AP2]).await;
    let plan = fleet.jobs.plan().await.unwrap();
    assert!(plan.ready() && plan.changes());
    let order: Vec<NodeRole> = plan.nodes.iter().map(|node| node.role).collect();
    assert_eq!(order, [NodeRole::Ap, NodeRole::Ap, NodeRole::Gateway]);

    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(
        (job.kind, job.revision),
        (JobKind::Apply, Some(fleet.head().await.meta.id))
    );
    assert!(states(&job).iter().all(|(name, state)| *state
        == if *name == StepName::Install {
            StepState::Skipped
        } else {
            StepState::Done
        }));
    assert_eq!(node_states(&job), [JobNodeState::Confirmed; 3]);

    // Every node staged and checked before any applied; the gateway applied last.
    let journal = fleet.journal();
    let first_apply = journal
        .iter()
        .position(|line| line.ends_with(" apply"))
        .unwrap();
    let last_check = journal
        .iter()
        .rposition(|line| line.ends_with(" check"))
        .unwrap();
    assert!(last_check < first_apply, "{journal:#?}");
    let applies: Vec<&str> = journal
        .iter()
        .filter(|line| line.ends_with(" apply"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        applies,
        [
            "192.168.1.21 apply",
            "192.168.1.22 apply",
            "192.168.1.1 apply"
        ]
    );
    assert_eq!(
        journal
            .iter()
            .filter(|line| line.ends_with(" confirm"))
            .count(),
        3
    );

    let ap = fleet.fake(1);
    assert!(ap.file("network").unwrap().contains("proto 'dhcp'"));
    assert!(!ap.tool.lock().unwrap().services["dnsmasq"]);
    assert!(fleet.fake(0).tool.lock().unwrap().services["dnsmasq"]);
    // The gateway resolves uniwrt.lan to where the controller logged in from.
    let dhcp = fleet.fake(0).file("dhcp").unwrap();
    assert!(
        dhcp.contains("option name 'uniwrt'\n\toption ip '192.168.1.50'"),
        "{dhcp}"
    );
    assert_eq!(last(&fleet, 0), (Outcome::Confirmed, None));
    let records = fleet.store.applied_revisions().await.unwrap();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|record| record.job == job.id));
    // Each node's managed packages are confirmed as it holds them now: drift's baseline.
    let confirmed = fleet
        .store
        .confirmed(Some(fleet.nodes[1].0.id))
        .await
        .unwrap();
    assert_ne!(confirmed, []);
    for package in &confirmed {
        assert_eq!(
            ap.file(&package.package).as_deref(),
            Some(package.text.as_str())
        );
    }

    // Applying again finds every node holding the revision.
    let again = fleet.apply().await;
    assert_eq!(again.state, JobState::Succeeded);
    assert_eq!(node_states(&again), [JobNodeState::Unchanged; 3]);
    let skipped: Vec<StepName> = again
        .steps
        .iter()
        .filter(|step| step.state == StepState::Skipped)
        .map(|step| step.name)
        .collect();
    assert_eq!(
        skipped,
        [
            StepName::Install,
            StepName::Stage,
            StepName::Apply,
            StepName::Verify,
            StepName::Confirm
        ]
    );
}

#[tokio::test]
async fn a_node_that_refuses_its_stage_changes_nothing_anywhere() {
    let fleet = fleet(&[GW, AP1]).await;
    *fleet.fake(0).refuse.lock().unwrap() = Some("check");
    let job = fleet.apply().await;
    let message = failed_with(&job, JobErrorCode::NodeFailed);
    assert!(
        message.starts_with("Nothing changed. gw-0: It refused"),
        "{message}"
    );
    assert!(!fleet.journal().iter().any(|line| line.ends_with(" apply")));
    assert_eq!(fleet.fake(1).file("network"), None);
    let by_node = |name: &str| {
        job.nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap()
            .state
    };
    assert_eq!(by_node("gw-0"), JobNodeState::Failed);
    assert_eq!(by_node("ap-1"), JobNodeState::RolledBack);
    assert_eq!(fleet.store.applied_revisions().await.unwrap(), []);
}

#[tokio::test]
async fn a_failed_health_check_rolls_every_node_back() {
    let fleet = fleet(&[GW, AP1]).await;
    *fleet.fake(0).fault.lock().unwrap() = Some(Fault::DnsDown);
    let job = fleet.apply().await;
    let message = failed_with(&job, JobErrorCode::RolledBack);
    assert!(
        message.contains("dnsmasq doesn't resolve localhost"),
        "{message}"
    );
    for i in 0..2 {
        assert_eq!(
            last(&fleet, i),
            (Outcome::RolledBack, Some(Reason::Requested))
        );
        assert_eq!(fleet.fake(i).file("network"), None);
    }
    assert!(fleet.fake(1).tool.lock().unwrap().services["dnsmasq"]);
    let by_node = |name: &str| {
        job.nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap()
            .state
    };
    assert_eq!(by_node("gw-0"), JobNodeState::RolledBack);
    assert!(
        fleet
            .journal()
            .iter()
            .all(|line| !line.ends_with(" confirm"))
    );
}

#[tokio::test]
async fn a_node_lost_after_applying_rolls_back_at_its_deadline() {
    let fleet = fleet(&[GW, AP1]).await;
    *fleet.fake(1).fault.lock().unwrap() = Some(Fault::Unreachable);
    let job = fleet.apply().await;
    let message = failed_with(&job, JobErrorCode::RolledBack);
    assert!(
        message.contains("ap-1: Unreachable at 192.168.1.21"),
        "{message}"
    );
    assert_eq!(
        last(&fleet, 0),
        (Outcome::RolledBack, Some(Reason::Requested))
    );
    let ap = job.nodes.iter().find(|node| node.name == "ap-1").unwrap();
    assert_eq!(ap.state, JobNodeState::RolledBack);
    assert!(
        ap.detail
            .as_deref()
            .unwrap()
            .starts_with("Unreachable: it rolls back by itself")
    );
    // Its timer runs out: it restores what it had.
    fleet.fake(1).uptime.fetch_add(121, Ordering::SeqCst);
    assert_eq!(
        last(&fleet, 1),
        (Outcome::RolledBack, Some(Reason::Timeout))
    );
    assert_eq!(fleet.fake(1).file("network"), None);
}

#[tokio::test]
async fn a_confirm_lost_after_others_succeeded_leaves_a_partial_apply() {
    let fleet = fleet(&[GW, AP1]).await;
    *fleet.fake(0).fault.lock().unwrap() = Some(Fault::Late);
    let job = fleet.apply().await;
    let message = failed_with(&job, JobErrorCode::PartialApply);
    assert!(
        message.contains("gw-0: It rolled back by itself"),
        "{message}"
    );
    assert_eq!(last(&fleet, 1), (Outcome::Confirmed, None));
    assert_eq!(
        last(&fleet, 0),
        (Outcome::RolledBack, Some(Reason::Timeout))
    );
    let applied = fleet.store.applied_revisions().await.unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].node, fleet.nodes[1].0.id);
}

#[tokio::test]
async fn a_lost_first_confirm_rolls_the_rest_back() {
    let fleet = fleet(&[GW, AP1]).await;
    *fleet.fake(1).fault.lock().unwrap() = Some(Fault::Late);
    let job = fleet.apply().await;
    failed_with(&job, JobErrorCode::RolledBack);
    assert_eq!(
        last(&fleet, 0),
        (Outcome::RolledBack, Some(Reason::Requested))
    );
    assert_eq!(fleet.store.applied_revisions().await.unwrap(), []);
}

#[tokio::test]
async fn unreachable_or_refused_nodes_stop_it_and_observed_ones_are_skipped() {
    let fleet = fleet(&[GW, AP1, AP2]).await;
    fleet
        .store
        .set_node_mode(fleet.nodes[2].0.id, NodeMode::Observe)
        .await
        .unwrap();
    fleet.fake(1).down.store(true, Ordering::SeqCst);
    let job = fleet.apply().await;
    let message = failed_with(&job, JobErrorCode::Unreachable);
    assert!(
        message.starts_with("Nothing changed. ap-1: Cannot log in"),
        "{message}"
    );
    assert_eq!(job.nodes[2].state, JobNodeState::Skipped);
    assert_eq!(fleet.journal(), Vec::<String>::new());

    fleet.fake(1).down.store(false, Ordering::SeqCst);
    // A PPPoE WAN without its password in the vault: render refuses the gateway.
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.wan.proto = uniwrt_model::site::gateway::WanProto::Pppoe {
        username: "user@isp".to_owned(),
        password: None,
        service: None,
    };
    commit(&fleet.store, Some(head.meta.id), site).await;
    let plan = fleet.jobs.plan().await.unwrap();
    assert!(!plan.ready());
    let skipped = plan.nodes.last().unwrap();
    assert_eq!(
        (skipped.name.as_str(), skipped.action),
        ("ap-2", PlanAction::Skipped)
    );
    let job = fleet.apply().await;
    failed_with(&job, JobErrorCode::Refused);
    assert_eq!(fleet.journal(), Vec::<String>::new());
}

#[tokio::test]
async fn the_gateway_moves_to_its_new_address_and_is_watched_there() {
    let fleet = fleet(&[GW]).await;
    let head = fleet.head().await;
    let mut site = head.config.clone();
    let lan = site.networks[0].ipv4.as_mut().unwrap();
    lan.address = "10.0.0.1/24".parse().unwrap();
    let dhcp = lan.dhcp.as_mut().unwrap();
    dhcp.start = "10.0.0.100".parse().unwrap();
    dhcp.end = "10.0.0.249".parse().unwrap();
    commit(&fleet.store, Some(head.meta.id), site).await;
    fleet
        .fake(0)
        .hosts
        .lock()
        .unwrap()
        .push("10.0.0.1".to_owned());
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    let node = fleet
        .store
        .node(fleet.nodes[0].0.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node.host, "10.0.0.1");
    let moved = fleet.watched.1.lock().unwrap().clone();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].host, "10.0.0.1");
}

#[tokio::test]
async fn only_the_latest_revision_applies_and_one_apply_at_a_time() {
    let fleet = fleet(&[GW]).await;
    let head = fleet.head().await.meta.id;
    let stale = commit(&fleet.store, Some(head), fleet.head().await.config).await;
    assert_ne!(stale, head);
    assert!(matches!(
        fleet.jobs.apply(head, None).await,
        Err(Error::Stale { .. })
    ));
    let started = fleet.jobs.apply(stale, None).await.unwrap();
    assert!(matches!(
        fleet.jobs.apply(stale, None).await,
        Err(Error::Busy { .. })
    ));
    assert!(matches!(
        fleet.jobs.manage(fleet.nodes[0].0.id, false, None).await,
        Err(Error::Busy { .. })
    ));
    let _ = started;
}

//! The backup and restore jobs (D-126) over the scripted fleet, which runs the simulated
//! transaction tool.

use uniwrt_model::{
    inventory::BackupReason,
    job::{JobErrorCode, JobKind, JobNodeState, JobState, StepName, StepState},
    node::NodeRole,
};
use uniwrt_store::BackupRepo as _;

use crate::{
    Error,
    apply::tests::fleet::{Fault, Fleet, fleet},
};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

/// A gateway holding its first revision, with the backup the apply took before it.
async fn applied() -> Fleet {
    let fleet = fleet(&[GW]).await;
    assert_eq!(fleet.apply().await.state, JobState::Succeeded);
    fleet
}

#[tokio::test]
async fn a_backup_job_keeps_each_node_s_backup_and_an_unchanged_one_once() {
    let fleet = applied().await;
    let ids: Vec<_> = fleet.nodes.iter().map(|(node, _)| node.id).collect();
    let gw = ids[0];
    let job = fleet
        .ends(fleet.jobs.back_up(&ids, BackupReason::Manual, None))
        .await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!((job.kind, job.node), (JobKind::Backup, Some(gw)));
    assert_eq!(job.nodes[0].state, JobNodeState::BackedUp);
    assert!(job.nodes[0].detail.as_deref().unwrap().contains("sealed"));

    let again = fleet
        .ends(fleet.jobs.back_up(&ids, BackupReason::Scheduled, None))
        .await;
    assert!(
        again.nodes[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("Unchanged since its last backup"),
        "{:?}",
        again.nodes[0].detail
    );
    let reasons: Vec<_> = fleet
        .store
        .backups(gw)
        .await
        .unwrap()
        .iter()
        .map(|backup| backup.reason)
        .collect();
    assert_eq!(reasons, [BackupReason::Manual, BackupReason::Apply]);
}

#[tokio::test]
async fn a_node_that_fails_doesn_t_stop_the_others() {
    let fleet = fleet(&[GW, AP]).await;
    let ids: Vec<_> = fleet.nodes.iter().map(|(node, _)| node.id).collect();
    fleet
        .fake(1)
        .down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let job = fleet
        .ends(fleet.jobs.back_up(&ids, BackupReason::Scheduled, None))
        .await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error.message.ends_with("The other 1 node backed up"),
        "{}",
        error.message
    );
    let states: Vec<_> = job.nodes.iter().map(|node| node.state).collect();
    assert_eq!(states, [JobNodeState::BackedUp, JobNodeState::Failed]);
}

#[tokio::test]
async fn a_restore_puts_the_backup_s_packages_back_as_a_confirmed_transaction() {
    let fleet = applied().await;
    let gw = fleet.nodes[0].0.id;
    let backups = fleet.store.backups(gw).await.unwrap();
    let before = backups[0].clone();
    assert_eq!(before.reason, BackupReason::Apply);
    let applied = fleet.fake(0).file("network").unwrap();

    let job = fleet.ends(fleet.jobs.restore(gw, before.id, None)).await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(job.kind, JobKind::Restore);
    assert!(job.steps.iter().all(|step| step.state == StepState::Done));
    let restored = fleet.fake(0).file("network").unwrap();
    assert_ne!(restored, applied, "the apply's network is replaced");
    let journal = fleet.journal();
    let restore: Vec<&str> = journal[journal.len() - 5..]
        .iter()
        .filter_map(|line| line.split(' ').nth(1))
        .collect();
    assert_eq!(restore, ["stage", "put", "check", "apply", "confirm"]);
    // What it held before the restore was backed up first.
    let reasons: Vec<_> = fleet
        .store
        .backups(gw)
        .await
        .unwrap()
        .iter()
        .map(|backup| backup.reason)
        .collect();
    assert_eq!(reasons, [BackupReason::Restore, BackupReason::Apply]);

    // Again: it holds the backup's packages already.
    let again = fleet.ends(fleet.jobs.restore(gw, before.id, None)).await;
    assert_eq!(again.state, JobState::Succeeded);
    let skipped: Vec<_> = again
        .steps
        .iter()
        .filter(|step| step.state == StepState::Skipped)
        .map(|step| step.name)
        .collect();
    assert_eq!(skipped[0], StepName::Backup);
    assert_eq!(fleet.fake(0).file("network").unwrap(), restored);
}

#[tokio::test]
async fn a_restore_that_hurts_the_node_s_health_rolls_back() {
    let fleet = applied().await;
    let gw = fleet.nodes[0].0.id;
    let before = fleet.store.backups(gw).await.unwrap()[0].id;
    let applied = fleet.fake(0).file("network").unwrap();
    *fleet.fake(0).fault.lock().unwrap() = Some(Fault::DnsDown);

    let job = fleet.ends(fleet.jobs.restore(gw, before, None)).await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::RolledBack, "{}", error.message);
    assert!(
        error
            .message
            .contains("it was rolled back and holds what it had"),
        "{}",
        error.message
    );
    assert_eq!(fleet.fake(0).file("network").unwrap(), applied);
}

#[tokio::test]
async fn a_restore_needs_management_and_the_node_s_own_backup() {
    let fleet = applied().await;
    let gw = fleet.nodes[0].0.id;
    let missing = uniwrt_core::BackupId::new(999);
    assert!(matches!(
        fleet.jobs.restore(gw, missing, None).await,
        Err(Error::NoBackup(node, backup)) if node == gw && backup == missing
    ));
}

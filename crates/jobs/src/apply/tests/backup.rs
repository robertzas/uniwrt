//! The apply's Backup step (D-126) over the scripted fleet.

use uniwrt_model::{
    inventory::BackupReason,
    job::{JobErrorCode, JobNodeState, JobState, StepName, StepState},
    node::NodeRole,
};
use uniwrt_store::BackupRepo as _;

use super::fleet::{fleet, output};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

#[tokio::test]
async fn a_node_that_cannot_be_backed_up_stops_the_apply_before_anything_changes() {
    let fleet = fleet(&[GW, AP]).await;
    *fleet.fake(1).exec_hook.lock().unwrap() = Some(Box::new(|_, argv| {
        (argv[0] == "sysupgrade").then(|| {
            let mut out = output(1, "");
            out.stderr = b"tar: short write".to_vec();
            out
        })
    }));
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error.message.starts_with("Nothing changed")
            && error.message.contains("ap-1: Cannot take a backup"),
        "{}",
        error.message
    );
    let failed: Vec<_> = job
        .nodes
        .iter()
        .filter(|node| node.state == JobNodeState::Failed)
        .map(|node| node.name.as_str())
        .collect();
    assert_eq!(failed, ["ap-1"]);
    assert!(
        fleet.journal().iter().all(|line| !line.contains(" stage")),
        "nothing was staged: {:?}",
        fleet.journal()
    );
    // The gateway's backup is kept all the same.
    let gateway = &fleet.nodes[0].0;
    assert_eq!(fleet.store.backups(gateway.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn each_node_that_changes_is_backed_up_before_and_none_that_doesn_t() {
    let fleet = fleet(&[GW, AP]).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    let step = job
        .steps
        .iter()
        .find(|s| s.name == StepName::Backup)
        .unwrap();
    assert_eq!(step.state, StepState::Done);
    assert_eq!(
        step.detail.as_deref(),
        Some("The 2 nodes to change are backed up")
    );
    for (node, _) in &fleet.nodes {
        let backups = fleet.store.backups(node.id).await.unwrap();
        let reasons: Vec<_> = backups.iter().map(|b| b.reason).collect();
        assert_eq!(reasons, [BackupReason::Apply], "{}", node.name);
    }
    // Nothing changes: nothing is backed up.
    let again = fleet.apply().await;
    let step = again
        .steps
        .iter()
        .find(|s| s.name == StepName::Backup)
        .unwrap();
    assert_eq!(step.state, StepState::Skipped);
    let gateway = &fleet.nodes[0].0;
    assert_eq!(fleet.store.backups(gateway.id).await.unwrap().len(), 1);
}

//! The mesh's synchronized cutover over the scripted fleet (M6.1 b2, D-114): a change that
//! moves the mesh applies on every node with one countdown, rather than leaves → gateway; a
//! mesh that only joins applies as before; a node refusing its apply rolls the others back
//! before the instant.

use uniwrt_model::{
    job::{Job, JobErrorCode, JobNodeState, JobState, StepName, StepState},
    node::NodeRole,
};

use super::{
    fleet::{Fleet, commit, commit_sealed, fleet},
    radios::meshed,
};
use crate::testing::states;

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

/// The gateway and an access point with hwsim-b's radios, meshed by an apply that joined them.
async fn meshing() -> Fleet {
    let fleet = fleet(&[GW, AP]).await;
    for i in 0..2 {
        meshed(fleet.fake(i));
    }
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.mesh.enabled = true;
    let key = fleet
        .vault
        .seal_config_secret(b"mesh-key-of-the-test")
        .unwrap();
    site.mesh.key = Some(key.reference().clone());
    commit_sealed(&fleet.store, Some(head.meta.id), site, vec![key]).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    for i in 0..2 {
        assert!(
            fleet
                .fake(i)
                .file("wireless")
                .unwrap()
                .contains("'uniwrt-mesh'")
        );
        // Joining drops no link: leaves → gateway, without a countdown.
        assert_eq!(fleet.fake(i).tool.lock().unwrap().waits, [0; 0]);
    }
    fleet
}

/// Commits the mesh under another ID: every mesh point moves.
async fn rename(fleet: &Fleet) {
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.mesh.mesh_id = "uniwrt-moved".to_owned();
    commit(&fleet.store, Some(head.meta.id), site).await;
}

fn step(job: &Job, name: StepName) -> (StepState, String) {
    let step = job.steps.iter().find(|s| s.name == name).unwrap();
    (step.state, step.detail.clone().unwrap_or_default())
}

#[tokio::test]
async fn a_moving_mesh_reloads_every_node_at_one_instant() {
    let fleet = meshing().await;
    rename(&fleet).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    let (state, detail) = step(&job, StepName::Apply);
    assert_eq!(state, StepState::Done);
    assert!(detail.contains("reloaded together"), "{detail}");
    for i in 0..2 {
        let tool = fleet.fake(i).tool.lock().unwrap();
        // One countdown each, within the lead (50 ms in the tests).
        assert_eq!(tool.waits.len(), 1, "{:?}", tool.waits);
        assert!(tool.waits[0] <= 50, "{:?}", tool.waits);
        assert!(tool.files["wireless"].contains("'uniwrt-moved'"));
    }
    assert!(states(&job).contains(&(StepName::Confirm, StepState::Done)));
}

#[tokio::test]
async fn a_node_refusing_the_cutover_rolls_the_others_back_before_it() {
    let fleet = meshing().await;
    let held = fleet.fake(0).file("wireless").unwrap();
    rename(&fleet).await;
    *fleet.fake(1).refuse.lock().unwrap() = Some("apply");
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.as_ref().unwrap();
    assert_eq!(error.code, JobErrorCode::RolledBack, "{}", error.message);
    assert!(
        error
            .message
            .starts_with("Rolled back before the mesh moved"),
        "{}",
        error.message
    );
    // The gateway got its countdown, then the rollback.
    assert_eq!(fleet.fake(0).tool.lock().unwrap().waits.len(), 1);
    assert_eq!(fleet.fake(0).file("wireless").unwrap(), held);
    let node = |i: usize| {
        job.nodes
            .iter()
            .find(|n| n.node == fleet.nodes[i].0.id)
            .unwrap()
            .state
    };
    assert_eq!(node(0), JobNodeState::RolledBack);
    assert_eq!(node(1), JobNodeState::Failed);
}

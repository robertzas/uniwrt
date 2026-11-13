//! The apply a port adoption ends with (D-107): the revision the gateway holds, applied to the
//! new node alone as an apply job of its own; refused while another apply runs.

use uniwrt_model::{
    job::{JobKind, JobNodeState, JobState},
    node::{NodeMode, NodeRole},
};
use uniwrt_store::{ApplyRepo as _, JobRepo as _, NodeRepo as _};

use super::fleet::{commit, fleet};
use crate::apply::{self, Configured};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

#[tokio::test]
async fn the_new_node_gets_the_revision_the_gateway_holds_and_nothing_else_changes() {
    let fleet = fleet(&[GW, AP]).await;
    let (gw, ap) = (fleet.nodes[0].0.id, fleet.nodes[1].0.id);
    // The gateway holds the first revision; the AP, in observe mode then, holds nothing.
    fleet
        .store
        .set_node_mode(ap, NodeMode::Observe)
        .await
        .unwrap();
    let held = fleet.head().await;
    assert_eq!(fleet.apply().await.state, JobState::Succeeded);
    let node = fleet
        .store
        .set_node_mode(ap, NodeMode::Managed)
        .await
        .unwrap()
        .unwrap();
    // An edit not applied yet stays out.
    commit(&fleet.store, Some(held.meta.id), held.config.clone()).await;
    fleet.journal.lock().unwrap().clear();

    let Configured {
        job,
        revision,
        result,
    } = apply::adopted(&fleet.jobs.shared, gw, node, None)
        .await
        .expect("an apply starts");
    assert_eq!(result, Ok(()));
    assert_eq!(revision, held.meta.id);
    let job = fleet.store.job(job).await.unwrap().unwrap();
    assert_eq!(
        (job.kind, job.state, job.revision),
        (JobKind::Apply, JobState::Succeeded, Some(held.meta.id))
    );
    let nodes: Vec<_> = job.nodes.iter().map(|n| (n.node, n.state)).collect();
    assert_eq!(nodes, [(ap, JobNodeState::Confirmed)]);
    let journal = fleet.journal();
    assert_ne!(journal, Vec::<String>::new());
    assert!(
        journal.iter().all(|line| line.starts_with(AP.1)),
        "{journal:?}"
    );
    let applied = fleet.store.applied_revisions().await.unwrap();
    let mut holds: Vec<_> = applied.iter().map(|a| (a.node, a.revision)).collect();
    holds.sort();
    assert_eq!(holds, [(gw, held.meta.id), (ap, held.meta.id)]);
    // Its targets are released: the next apply starts.
    assert!(!fleet.jobs.shared.busy(apply::TARGET));
}

#[tokio::test]
async fn no_apply_starts_while_another_runs_or_before_the_gateway_holds_a_revision() {
    let fleet = fleet(&[GW, AP]).await;
    let (gw, node) = (fleet.nodes[0].0.id, fleet.nodes[1].0.clone());
    let why = apply::adopted(&fleet.jobs.shared, gw, node.clone(), None)
        .await
        .err();
    assert_eq!(
        why.as_deref(),
        Some("The gateway holds no revision to apply")
    );

    assert_eq!(fleet.apply().await.state, JobState::Succeeded);
    let head = fleet.head().await;
    let hold = apply::job(&head, &[], None);
    let _held = fleet
        .jobs
        .shared
        .claim(vec![apply::TARGET.to_owned()], hold)
        .await
        .unwrap();
    let why = apply::adopted(&fleet.jobs.shared, gw, node, None)
        .await
        .err();
    assert_eq!(why.as_deref(), Some("Another apply is running"));
}

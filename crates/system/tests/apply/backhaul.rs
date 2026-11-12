//! The apply's backhaul round on OpenWrt (features/mesh.md, Wired backhaul; D-097): with the
//! mesh on, the harness's one cable between the gateway's LAN port and the AP's (`eth0` at both
//! ends) comes over as the apply stages it. The first transaction adds batman-adv on the cable's
//! VLAN beside the LAN; the Backhaul step waits for each end to hear the other over it and runs
//! the agent's MTU probe from both (through `Remote::call`); the round's transaction then takes
//! the port out of each bridge. The cable is the AP's only path, so its LAN reaches the gateway
//! across `bat0` from then on, and a break before the make would have cut it off.

use std::time::Duration;

use uniwrt_model::{
    apply::PlanAction,
    job::{JobState, StepName, StepState},
    site::revision::{NewRevision, RevisionKind},
};
use uniwrt_node::txn;
use uniwrt_store::RevisionRepo as _;
use uniwrt_system::{Node, needs};

use super::{AP_CONTAINER, GW, UP, in_subnet, lab, wait_status};

const PORT: &str = "eth0";

const CONTEXT: &str =
    "batctl meshif bat0 n; batctl meshif bat0 if; bridge link; ip -br link; logread | tail -n 30";

/// `node`'s end of the cable: its VLAN in batman-adv, which hears the far end over it, and the
/// port out of the bridge.
fn exclusive(node: &Node, vlan: &str) {
    let neighbors = node.sh("batctl meshif bat0 n -H", &[]);
    assert!(
        neighbors.lines().any(|line| line.contains(vlan)),
        "{}: no neighbor over {vlan}:\n{}",
        node.container,
        node.sh(CONTEXT, &[])
    );
    assert!(
        !node.bridge_vlans().contains_key(PORT),
        "{}: {PORT} is still bridged:\n{}",
        node.container,
        node.sh(CONTEXT, &[])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs scripts/test-system.sh's topology"]
async fn the_cable_comes_over_to_the_wired_backhaul() {
    if !needs("batman-adv") {
        return;
    }
    let lab = lab().await;
    let mut config = lab.base.clone();
    let key = lab
        .vault
        .seal_config_secret(b"system mesh key 0123")
        .expect("the key seals");
    config.mesh.enabled = true;
    config.mesh.wired = true;
    config.mesh.key = Some(key.reference().clone());
    let head = lab.store.head_revision().await.expect("head").expect("one");
    let revision = lab
        .store
        .commit_revision(
            NewRevision {
                base: Some(head.meta.id),
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: None,
                author: None,
                message: "the wired backhaul".to_owned(),
                config,
            },
            vec![key],
        )
        .await
        .expect("the revision commits")
        .id;
    let plan = lab.jobs.plan().await.expect("a plan");
    assert!(plan.ready(), "{plan:#?}");

    let job = lab.apply(revision).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let round = job
        .steps
        .iter()
        .find(|step| step.name == StepName::Backhaul)
        .expect("a backhaul round");
    assert_eq!(round.state, StepState::Done, "{job:#?}");
    let detail = round.detail.as_deref().unwrap_or_default();
    assert!(detail.contains("carries the mesh alone"), "{detail}");

    let vlan = format!("{PORT}.{}", lab.base.mesh.wired_vlan);
    let name = format!("uniwrt-{}-backhaul", job.id);
    for (node, container) in [(&lab.gw, GW), (&lab.ap, AP_CONTAINER)] {
        exclusive(node, &vlan);
        let status = wait_status(container, Duration::ZERO, |_| true).await;
        let last = status.last.expect("a finished transaction");
        assert_eq!(
            (status.open, last.id.as_str(), last.outcome),
            (None, name.as_str(), txn::Outcome::Confirmed),
            "{container}"
        );
    }
    // The AP's LAN holds its lease across bat0, and the gateway answers there.
    lab.ap
        .wait_interface("lan", UP, |s| in_subnet(s, "10.0.0."));
    lab.ap.sh("ping -c 3 -W 2 10.0.0.1 >/dev/null", &[]);
    // The cable held alone at both ends stays so: applying again changes nothing.
    let plan = lab.jobs.plan().await.expect("a plan");
    assert!(
        plan.nodes.iter().all(|n| n.action == PlanAction::Unchanged),
        "{plan:#?}"
    );
}

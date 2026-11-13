//! The wired backhaul's round (D-097) over the scripted fleet: a cable between the gateway and
//! an access point (each bridge sees the other node on `eth0`) comes over as an overlay, is
//! probed from both ends once batman-adv hears the far end on its VLAN, then carries the mesh
//! alone; a cable that fails goes back to a trunk and is left so.

use uniwrt_model::{
    job::{Job, JobState, StepName, StepState},
    node::NodeRole,
};
use uniwrt_store::BackhaulRepo as _;

use super::fleet::{Fleet, commit, fleet};
use crate::testing::states;

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

/// The gateway and an access point on one cable, both with batman-adv and the mesh on; each
/// hears the other over the cable's VLAN once its `network` has the backhaul there.
async fn cabled() -> Fleet {
    let fleet = fleet(&[GW, AP]).await;
    let macs: Vec<String> = fleet.nodes.iter().map(|(n, _)| n.mac.to_string()).collect();
    for (i, (_, fake)) in fleet.nodes.iter().enumerate() {
        let peer = macs[1 - i].clone();
        *fake.collect_hook.lock().unwrap() = Some(Box::new(move |tool, collected| {
            let sets = &mut collected["sets"];
            let caps = &mut sets["capabilities"];
            caps["software"]
                .as_array_mut()
                .unwrap()
                .push("batman-adv".into());
            caps["hostapd"] = serde_json::json!(["sae", "mesh"]);
            sets["neighbors"] = serde_json::json!({"rtnl": {"neigh": [], "fdb": [
                {"dev": "eth0", "master": "br-lan", "lladdr": peer}
            ]}});
            let network = tool.files.get("network").cloned().unwrap_or_default();
            let neighbors = if network.contains("'eth0.4094'") {
                serde_json::json!([{"hard_ifname": "eth0.4094", "neigh_address": peer,
                    "last_seen_msecs": 100}])
            } else {
                serde_json::json!([])
            };
            sets["batman"] = serde_json::json!({"bat0": {
                "mesh": {"mesh_address": null, "algo_name": "BATMAN_V", "gw_mode": "off"},
                "neighbors": neighbors,
            }});
        }));
    }
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.mesh.enabled = true;
    commit(&fleet.store, Some(head.meta.id), site).await;
    fleet
}

fn step(job: &Job, name: StepName) -> StepState {
    states(job)
        .into_iter()
        .find(|(step, _)| *step == name)
        .unwrap()
        .1
}

/// The confirms the node at `host` got since journal line `from`: one per transaction.
fn transactions(fleet: &Fleet, from: usize, host: &str) -> Vec<String> {
    fleet.journal()[from..]
        .iter()
        .filter_map(|line| line.strip_prefix(&format!("{host} ")))
        .filter(|command| *command == "confirm")
        .map(str::to_owned)
        .collect()
}

fn bridged(fleet: &Fleet, i: usize) -> bool {
    let network = fleet.fake(i).file("network").unwrap();
    let bridge = network
        .split("config ")
        .find(|s| s.contains("'br-lan'") && s.contains("ports"))
        .unwrap();
    bridge.contains("'eth0'")
}

#[tokio::test]
async fn a_cable_comes_over_then_carries_the_mesh_alone() {
    let fleet = cabled().await;
    let plan = fleet.jobs.plan().await.unwrap();
    for node in &plan.nodes {
        assert!(
            node.warnings
                .iter()
                .any(|w| w.path == "mesh/wired" && w.message.starts_with("eth0's cable to")),
            "{:?}",
            node.warnings
        );
    }
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(step(&job, StepName::Backhaul), StepState::Done);
    let detail = job
        .steps
        .iter()
        .find(|s| s.name == StepName::Backhaul)
        .and_then(|s| s.detail.clone())
        .unwrap();
    assert!(
        detail.contains("gw-0 eth0 to ap-1 eth0 carries the mesh alone"),
        "{detail}"
    );
    for i in 0..2 {
        let network = fleet.fake(i).file("network").unwrap();
        assert!(
            network.contains("config interface 'uw_bh_eth0'"),
            "{network}"
        );
        assert!(!bridged(&fleet, i), "{network}");
    }
    // Both hold it now: the next apply has nothing to do.
    let from = fleet.journal().len();
    let again = fleet.apply().await;
    assert_eq!(again.state, JobState::Succeeded, "{:?}", again.error);
    assert_eq!(step(&again, StepName::Backhaul), StepState::Skipped);
    assert_eq!(
        transactions(&fleet, from, "192.168.1.1"),
        Vec::<String>::new()
    );
    assert_eq!(fleet.store.backhaul_failures().await.unwrap(), []);
}

#[tokio::test]
async fn a_cable_that_fails_the_probe_goes_back_to_a_trunk_and_stays_one() {
    let fleet = cabled().await;
    *fleet.fake(1).mtu.lock().unwrap() = 1500;
    let from = fleet.journal().len();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    // The overlay, then back to a trunk.
    assert_eq!(transactions(&fleet, from, "192.168.1.21").len(), 2);
    for i in 0..2 {
        let network = fleet.fake(i).file("network").unwrap();
        assert!(!network.contains("uw_bh_eth0"), "{network}");
        assert!(bridged(&fleet, i), "{network}");
    }
    let failures = fleet.store.backhaul_failures().await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].why, "ap-1's eth0 takes 1500 bytes, not 1532");
    // Left a trunk: nothing to do, and no warning about it.
    let plan = fleet.jobs.plan().await.unwrap();
    assert!(
        plan.nodes
            .iter()
            .all(|n| n.warnings.iter().all(|w| w.path != "mesh/wired"))
    );
    let from = fleet.journal().len();
    let again = fleet.apply().await;
    assert_eq!(again.state, JobState::Succeeded, "{:?}", again.error);
    assert_eq!(
        transactions(&fleet, from, "192.168.1.21"),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn a_cable_whose_far_end_stays_silent_goes_back_to_a_trunk() {
    let fleet = cabled().await;
    // The access point never hears the gateway over the VLAN (a switch drops it, say).
    *fleet.fake(1).collect_hook.lock().unwrap() = Some(Box::new(|_, collected| {
        let sets = &mut collected["sets"];
        sets["capabilities"]["software"]
            .as_array_mut()
            .unwrap()
            .push("batman-adv".into());
        sets["capabilities"]["hostapd"] = serde_json::json!(["sae", "mesh"]);
        sets["neighbors"] = serde_json::json!({"rtnl": {"neigh": [], "fdb": [
            {"dev": "eth0", "master": "br-lan", "lladdr": "02:00:00:00:01:00"}
        ]}});
    }));
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    let failures = fleet.store.backhaul_failures().await.unwrap();
    assert_eq!(
        failures.iter().map(|f| f.why.as_str()).collect::<Vec<_>>(),
        ["batman-adv on ap-1's eth0 heard no node over the cable"]
    );
    assert!(bridged(&fleet, 0) && bridged(&fleet, 1));
}

#[tokio::test]
async fn turning_the_mesh_off_brings_the_cable_back_as_a_trunk_without_missing_its_neighbors() {
    let fleet = cabled().await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert!(!bridged(&fleet, 0));
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.mesh.enabled = false;
    commit(&fleet.store, Some(head.meta.id), site).await;
    // Neither node reaches the gateway over the air: nothing is refused or warned of.
    let plan = fleet.jobs.plan().await.unwrap();
    assert!(
        plan.nodes
            .iter()
            .all(|n| n.issues.is_empty() && n.warnings.iter().all(|w| w.path != "mesh")),
        "{plan:?}"
    );
    let off = fleet.apply().await;
    assert_eq!(off.state, JobState::Succeeded, "{:?}", off.error);
    for i in 0..2 {
        let network = fleet.fake(i).file("network").unwrap();
        assert!(!network.contains("batadv"), "{network}");
        assert!(bridged(&fleet, i), "{network}");
    }
}

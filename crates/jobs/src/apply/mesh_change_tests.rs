use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use uniwrt_core::NodeId;
use uniwrt_model::{
    apply::{NodePlan, PlanAction},
    node::NodeRole,
};
use uniwrt_uci::Package;

use super::{MeshChange, change, review};

fn network(algo: Option<&str>) -> Package {
    let algo = algo.map_or(String::new(), |a| format!("\toption routing_algo '{a}'\n"));
    uniwrt_uci::parse(
        "network",
        &format!("config interface 'bat0'\n\toption proto 'batadv'\n{algo}"),
    )
    .unwrap()
}

fn wireless(id: &str, key: &str, channel: &str) -> Package {
    uniwrt_uci::parse(
        "wireless",
        &format!(
            "config wifi-device 'radio1'\n\toption band '5g'\n\toption channel '{channel}'\n\n\
             config wifi-iface 'uw_mesh'\n\toption device 'radio1'\n\toption mode 'mesh'\n\
             \toption mesh_id '{id}'\n\toption key '{key}'\n"
        ),
    )
    .unwrap()
}

#[test]
fn a_mesh_point_comes_moves_or_goes_with_batman_adv_or_alone() {
    let (v, iv) = (network(Some("BATMAN_V")), network(None));
    let point = wireless("m", "k", "36");
    let on = (Some(&v), Some(&point));
    let none = (None, None);
    assert_eq!(change(on, on), MeshChange::Same);
    assert_eq!(change(none, none), MeshChange::Same);
    assert_eq!(change(none, on), MeshChange::Joins);
    assert_eq!(change(none, (Some(&v), None)), MeshChange::Joins);
    for other in [
        wireless("n", "k", "36"),
        wireless("m", "other", "36"),
        wireless("m", "k", "44"),
    ] {
        assert_eq!(change(on, (Some(&v), Some(&other))), MeshChange::Moves);
    }
    // No routing_algo is batman-adv's BATMAN_IV.
    assert_eq!(change(on, (Some(&iv), Some(&point))), MeshChange::Moves);
    assert_eq!(change(on, (Some(&v), None)), MeshChange::Leaves);
    assert_eq!(change(on, none), MeshChange::Off);
    assert_eq!(change((Some(&v), None), none), MeshChange::Off);
    let disabled = uniwrt_uci::parse(
        "wireless",
        "config wifi-device 'radio1'\n\toption disabled '1'\n\n\
         config wifi-iface 'uw_mesh'\n\toption device 'radio1'\n\toption mode 'mesh'\n",
    )
    .unwrap();
    assert_eq!(change(on, (Some(&v), Some(&disabled))), MeshChange::Leaves);
}

fn plan(id: i64, action: PlanAction) -> NodePlan {
    NodePlan {
        node: NodeId::new(id),
        name: format!("n{id}"),
        role: NodeRole::Ap,
        action,
        reason: None,
        packages: Vec::new(),
        drift: Vec::new(),
        raw: Vec::new(),
        services: Vec::new(),
        installs: Vec::new(),
        address: None,
        renumbering: None,
        wan_last: false,
        hops: None,
        timeout_s: 300,
        warnings: Vec::new(),
        issues: Vec::new(),
    }
}

fn reviewed(change_of: impl Fn(i64) -> MeshChange) -> Vec<NodePlan> {
    let mut plans = vec![
        plan(1, PlanAction::Change),
        plan(2, PlanAction::Change),
        plan(3, PlanAction::Change),
        plan(4, PlanAction::Skipped),
    ];
    let changes: BTreeMap<NodeId, MeshChange> =
        (1..=3).map(|id| (NodeId::new(id), change_of(id))).collect();
    let air: BTreeSet<NodeId> = [NodeId::new(3)].into();
    review(&mut plans, &changes, &air, Duration::from_secs(150));
    plans
}

#[test]
fn a_moving_mesh_warns_of_the_nodes_on_the_air_and_in_observe_mode() {
    let plans = reviewed(|_| MeshChange::Moves);
    assert!(plans[..2].iter().all(|p| p.warnings.is_empty()));
    assert_eq!(plans[2].action, PlanAction::Change);
    assert!(
        plans[2].warnings[0]
            .message
            .contains("drops off while the mesh changes")
    );
    assert!(plans[2].warnings[0].message.contains("up to 150 s"));
    assert!(plans[3].warnings[0].message.contains("observe mode"));
}

#[test]
fn a_mesh_going_off_refuses_the_nodes_it_would_strand() {
    let plans = reviewed(|_| MeshChange::Off);
    assert_eq!(plans[2].action, PlanAction::Refused);
    assert!(
        plans[2].issues[0]
            .message
            .contains("Connect it by cable first")
    );
    assert_eq!(plans[1].action, PlanAction::Change);
}

#[test]
fn a_mesh_that_only_comes_or_stays_warns_of_nothing() {
    for change in [MeshChange::Joins, MeshChange::Same] {
        let plans = reviewed(|_| change);
        assert!(
            plans
                .iter()
                .all(|p| p.warnings.is_empty() && p.issues.is_empty())
        );
    }
}

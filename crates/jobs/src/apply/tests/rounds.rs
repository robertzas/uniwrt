//! The gateway's later transactions (D-081) over the scripted fleet: the network this
//! controller sits on renumbers make-before-break, and the WAN changes last and alone.

use uniwrt_model::{
    apply::Renumbering,
    job::{Job, JobErrorCode, JobNodeState, JobState, StepName, StepState},
    node::NodeRole,
    site::{SiteConfig, network::Reservation},
};
use uniwrt_node::txn::TOOL;
use uniwrt_store::{ApplyRepo as _, DriftRepo as _, NodeRepo as _};

use super::fleet::{Fleet, commit, fleet, output};
use crate::testing::states;

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

/// A fleet holding its first revision.
async fn applied(members: &[(NodeRole, &str)]) -> Fleet {
    let fleet = fleet(members).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    fleet
}

/// Commits the head with `edit` on it.
async fn edit(fleet: &Fleet, edit: impl FnOnce(&mut SiteConfig)) {
    let head = fleet.head().await;
    let mut site = head.config.clone();
    edit(&mut site);
    commit(&fleet.store, Some(head.meta.id), site).await;
}

/// The LAN on 10.0.0.0/24, where the gateway and the access points answer too.
async fn renumber(fleet: &Fleet) {
    let aps: Vec<_> = fleet
        .nodes
        .iter()
        .skip(1)
        .map(|(node, _)| node.mac)
        .collect();
    edit(fleet, |site| {
        let lan = site.networks[0].ipv4.as_mut().unwrap();
        lan.address = "10.0.0.1/24".parse().unwrap();
        let dhcp = lan.dhcp.as_mut().unwrap();
        dhcp.start = "10.0.0.100".parse().unwrap();
        dhcp.end = "10.0.0.249".parse().unwrap();
        for (i, mac) in aps.into_iter().enumerate() {
            dhcp.reservations.push(Reservation {
                mac,
                ip: format!("10.0.0.{}", 21 + i).parse().unwrap(),
                name: None,
            });
        }
    })
    .await;
    for (i, (_, fake)) in fleet.nodes.iter().enumerate() {
        let host = if i == 0 {
            "10.0.0.1".to_owned()
        } else {
            format!("10.0.0.{}", 20 + i)
        };
        fake.hosts.lock().unwrap().push(host);
    }
}

/// The gateway's tool commands since `from`, every `put` and `extend` left out.
fn gateway_commands(fleet: &Fleet, from: usize) -> Vec<String> {
    fleet.journal()[from..]
        .iter()
        .filter_map(|line| line.strip_prefix("192.168.1.1 "))
        .filter(|command| !matches!(*command, "put" | "extend"))
        .map(str::to_owned)
        .collect()
}

fn step(job: &Job, name: StepName) -> StepState {
    states(job)
        .into_iter()
        .find(|(step, _)| *step == name)
        .unwrap()
        .1
}

async fn host(fleet: &Fleet, i: usize) -> String {
    let node = fleet.store.node(fleet.nodes[i].0.id).await.unwrap();
    node.unwrap().host
}

#[tokio::test]
async fn the_controllers_network_renumbers_make_before_break() {
    let fleet = applied(&[GW, AP]).await;
    renumber(&fleet).await;
    *fleet.fake(0).renews_to.lock().unwrap() = Some("10.0.0.50");
    let plan = fleet.jobs.plan().await.unwrap();
    let gateway = plan
        .nodes
        .iter()
        .find(|node| node.role == NodeRole::Gateway)
        .unwrap();
    assert_eq!(
        gateway.renumbering,
        Some(Renumbering {
            network: "lan".to_owned(),
            from: "192.168.1.0/24".to_owned(),
            to: "10.0.0.0/24".to_owned(),
        })
    );
    assert!(!gateway.wan_last);

    let from = fleet.journal().len();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(step(&job, StepName::Settle), StepState::Done);
    assert_eq!(step(&job, StepName::Wan), StepState::Skipped);
    assert!(
        job.nodes
            .iter()
            .all(|node| node.state == JobNodeState::Confirmed),
        "{:?}",
        job.nodes
    );
    // The bridge is applied, its ports bounce, and it is confirmed only once this controller
    // came from the new subnet; then the old address goes in a transaction of its own.
    assert_eq!(
        gateway_commands(&fleet, from),
        [
            "stage", "check", "apply", "bounce", "confirm", "stage", "check", "apply", "confirm"
        ]
    );
    let bounced = fleet.fake(0).tool.lock().unwrap().bounced.clone();
    assert_eq!(bounced.len(), 1);
    assert!(!bounced[0].is_empty() && !bounced[0].iter().any(|port| port == "eth1"));
    let network = fleet.fake(0).file("network").unwrap();
    assert!(
        network.contains("list ipaddr '10.0.0.1/24'") && !network.contains("192.168.1.1"),
        "{network}"
    );
    let dhcp = fleet.fake(0).file("dhcp").unwrap();
    assert!(!dhcp.contains("'2m'"), "{dhcp}");
    // uniwrt.lan follows the controller.
    assert!(
        dhcp.contains("option name 'uniwrt'\n\toption ip '10.0.0.50'"),
        "{dhcp}"
    );

    // Both are followed to their new addresses and hold the revision.
    assert_eq!(host(&fleet, 0).await, "10.0.0.1");
    assert_eq!(host(&fleet, 1).await, "10.0.0.21");
    let moved: Vec<String> = fleet
        .watched
        .1
        .lock()
        .unwrap()
        .iter()
        .map(|node| node.host.clone())
        .collect();
    assert_eq!(moved.len(), 2, "{moved:?}");
    let head = fleet.head().await.meta.id;
    let records = fleet.store.applied_revisions().await.unwrap();
    assert!(records.len() == 2 && records.iter().all(|record| record.revision == head));
    let again = fleet.apply().await;
    assert_eq!(again.state, JobState::Succeeded, "{:?}", again.error);
    assert!(
        again
            .nodes
            .iter()
            .all(|node| node.state == JobNodeState::Unchanged),
        "{:?}",
        again.nodes
    );
}

#[tokio::test]
async fn a_controller_that_keeps_its_address_rolls_the_renumbering_back() {
    let fleet = applied(&[GW]).await;
    let before = fleet.fake(0).tool.lock().unwrap().files.clone();
    let revision = fleet.head().await.meta.id;
    renumber(&fleet).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::RolledBack, "{}", error.message);
    assert!(
        error
            .message
            .contains("still reaches it from 192.168.1.50, outside 10.0.0.0/24"),
        "{}",
        error.message
    );
    assert_eq!(step(&job, StepName::Verify), StepState::Failed);
    assert_eq!(step(&job, StepName::Settle), StepState::Pending);
    // Rolled back, its ports bounced once more so the hosts return; the node stays where it was.
    let (files, bounces) = {
        let tool = fleet.fake(0).tool.lock().unwrap();
        (tool.files.clone(), tool.bounced.len())
    };
    assert_eq!((files, bounces), (before, 2));
    assert_eq!(host(&fleet, 0).await, "192.168.1.1");
    let records = fleet.store.applied_revisions().await.unwrap();
    assert_eq!(records[0].revision, revision);
}

#[tokio::test]
async fn an_access_point_that_stays_behind_rolls_everything_back() {
    let fleet = applied(&[GW, AP]).await;
    renumber(&fleet).await;
    *fleet.fake(0).renews_to.lock().unwrap() = Some("10.0.0.50");
    // It keeps answering at its old address only: verified there, it would be lost later.
    fleet.fake(1).hosts.lock().unwrap().pop();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::RolledBack, "{}", error.message);
    assert!(
        error.message.contains("ap-1: Unreachable at 10.0.0.21"),
        "{}",
        error.message
    );
    let network = fleet.fake(0).file("network").unwrap();
    assert!(
        network.contains("192.168.1.1/24") && !network.contains("10.0.0.1"),
        "{network}"
    );
    assert_eq!(fleet.fake(0).tool.lock().unwrap().bounced.len(), 2);
    assert_eq!(host(&fleet, 1).await, "192.168.1.21");
}

/// The WAN's MTU and the DHCP pool change together.
async fn wan_and_more(fleet: &Fleet) {
    edit(fleet, |site| {
        site.wan.mtu = Some(1492);
        let dhcp = site.networks[0]
            .ipv4
            .as_mut()
            .unwrap()
            .dhcp
            .as_mut()
            .unwrap();
        dhcp.end = "192.168.1.199".parse().unwrap();
    })
    .await;
}

#[tokio::test]
async fn the_wan_changes_last_in_its_own_transaction() {
    let fleet = applied(&[GW]).await;
    wan_and_more(&fleet).await;
    let plan = fleet.jobs.plan().await.unwrap();
    assert!(plan.nodes[0].wan_last && plan.nodes[0].renumbering.is_none());
    let from = fleet.journal().len();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(step(&job, StepName::Settle), StepState::Skipped);
    assert_eq!(step(&job, StepName::Wan), StepState::Done);
    assert_eq!(
        gateway_commands(&fleet, from),
        [
            "stage", "check", "apply", "confirm", "stage", "check", "apply", "confirm"
        ]
    );
    assert!(fleet.fake(0).file("network").unwrap().contains("1492"));
    assert!(
        fleet
            .fake(0)
            .file("dhcp")
            .unwrap()
            .contains("option limit '100'")
    );
    let head = fleet.head().await.meta.id;
    assert_eq!(
        fleet.store.applied_revisions().await.unwrap()[0].revision,
        head
    );
}

#[tokio::test]
async fn a_wan_change_that_fails_rolls_back_alone() {
    let fleet = applied(&[GW]).await;
    let revision = fleet.head().await.meta.id;
    wan_and_more(&fleet).await;
    // The gateway refuses the WAN's transaction, not the one before.
    *fleet.fake(0).exec_hook.lock().unwrap() = Some(Box::new(|_, argv| {
        let refused = argv[0] == TOOL && argv[1] == "check" && argv[2].ends_with("-wan");
        refused.then(|| output(1, r#"{"err":{"code":"invalid","msg":"uci: Parse error"}}"#))
    }));
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::PartialApply, "{}", error.message);
    assert!(
        error.message.starts_with(
            "The WAN's change failed and was undone; what was confirmed before it stays."
        ),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("gw-0: It refused the configuration"),
        "{}",
        error.message
    );
    assert_eq!(step(&job, StepName::Confirm), StepState::Done);
    assert_eq!(step(&job, StepName::Wan), StepState::Failed);
    // The pool changed and the WAN didn't; the gateway stays behind the revision, with what it
    // holds as drift's baseline.
    let network = fleet.fake(0).file("network").unwrap();
    let dhcp = fleet.fake(0).file("dhcp").unwrap();
    assert!(
        !network.contains("1492") && dhcp.contains("option limit '100'"),
        "{dhcp}"
    );
    assert_eq!(
        fleet.store.applied_revisions().await.unwrap()[0].revision,
        revision
    );
    let confirmed = fleet
        .store
        .confirmed(Some(fleet.nodes[0].0.id))
        .await
        .unwrap();
    let held = |package: &str| {
        confirmed
            .iter()
            .find(|held| held.package == package)
            .unwrap()
            .text
            .clone()
    };
    assert_eq!((held("network"), held("dhcp")), (network, dhcp));

    // Applying again, fixed, changes the WAN alone.
    *fleet.fake(0).exec_hook.lock().unwrap() = None;
    let from = fleet.journal().len();
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(
        gateway_commands(&fleet, from),
        ["stage", "check", "apply", "confirm"]
    );
    assert_eq!(step(&job, StepName::Wan), StepState::Skipped);
    assert!(fleet.fake(0).file("network").unwrap().contains("1492"));
}

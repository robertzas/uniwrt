//! The Channels round over the scripted fleet (D-113): access points whose radios' channels
//! alone change move after the rest, one at a time, by a channel switch, falling back to a
//! reload; their power goes with the fleet's transactions; a failure rolls back that node alone
//! and restarts its radios. A switched radio then catches up in the radio restart job (D-116).

use uniwrt_model::{
    job::{Job, JobErrorCode, JobNodeState, JobState, StepName, StepState},
    node::NodeRole,
    site::wireless::{PlannedRadio, TxPower},
    wifi::{Band, ChannelWidth},
};

use super::{
    fleet::{Fault, Fleet, commit, fleet},
    radios::{Switching, wifi},
};
use crate::testing::states;

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP1: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");
const AP2: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.22");

/// A gateway and access points with hwsim-b's radios, holding their first revision.
async fn applied(aps: &[(NodeRole, &str)]) -> Fleet {
    let mut members = vec![GW];
    members.extend_from_slice(aps);
    let fleet = fleet(&members).await;
    for i in 1..members.len() {
        wifi(fleet.fake(i));
    }
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    fleet
}

/// Commits a plan putting each access point's 2.4 GHz radio (`radio1`) on `channel`.
async fn plan(fleet: &Fleet, channel: u16, tx_power: TxPower) {
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.radio.plan.radios = fleet
        .nodes
        .iter()
        .filter(|(node, _)| node.role == NodeRole::Ap)
        .map(|(node, _)| PlannedRadio {
            node: node.id,
            radio: "radio1".to_owned(),
            band: Band::G2,
            channel,
            width: ChannelWidth::Mhz20,
            tx_power,
        })
        .collect();
    commit(&fleet.store, Some(head.meta.id), site).await;
}

fn step(job: &Job, name: StepName) -> (StepState, String) {
    let step = job.steps.iter().find(|s| s.name == name).unwrap();
    (step.state, step.detail.clone().unwrap_or_default())
}

fn node_detail(job: &Job, i: usize) -> (JobNodeState, String) {
    let node = &job.nodes[i];
    (node.state, node.detail.clone().unwrap_or_default())
}

#[tokio::test]
async fn channels_move_by_a_switch_one_access_point_at_a_time() {
    let fleet = applied(&[AP1, AP2]).await;
    let before = fleet.journal().len();
    plan(&fleet, 6, TxPower::Auto).await;
    let preview = fleet.jobs.plan().await.unwrap();
    assert!(
        preview
            .nodes
            .iter()
            .filter(|n| n.role == NodeRole::Ap)
            .all(|n| n.air)
    );
    assert!(
        preview
            .nodes
            .iter()
            .all(|n| n.role == NodeRole::Ap || !n.air)
    );
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    for name in [StepName::Stage, StepName::Apply, StepName::Confirm] {
        assert_eq!(step(&job, name).0, StepState::Skipped, "{name:?}");
    }
    let (state, detail) = step(&job, StepName::Channels);
    assert_eq!(state, StepState::Done);
    assert_eq!(
        detail,
        "Moved the channels of 2 nodes by a channel switch, one at a time"
    );

    // Each access point is staged, applied, verified and confirmed before the next one starts.
    let journal: Vec<String> = fleet.journal()[before..]
        .iter()
        .filter(|line| !line.ends_with(" put") && !line.ends_with(" check"))
        .cloned()
        .collect();
    assert_eq!(
        journal,
        [
            "192.168.1.21 stage",
            "192.168.1.21 apply",
            "192.168.1.21 extend",
            "192.168.1.21 confirm",
            "192.168.1.22 stage",
            "192.168.1.22 apply",
            "192.168.1.22 extend",
            "192.168.1.22 confirm",
        ]
    );
    for i in [1, 2] {
        let fake = fleet.fake(i);
        assert_eq!(fake.tool.lock().unwrap().live, [["radio1"]]);
        let radios = fake.radios.lock().unwrap();
        assert_eq!(radios.reloads, 0);
        let [(ifname, data)] = radios.switches.as_slice() else {
            panic!("{:?}", radios.switches)
        };
        assert_eq!(ifname, "phy4-ap0");
        assert_eq!(
            (data["freq"].as_u64(), data["bandwidth"].as_u64()),
            (Some(2437), Some(20))
        );
        assert!(
            fake.file("wireless")
                .unwrap()
                .contains("option channel '6'")
        );
        let (state, detail) = node_detail(&job, i);
        assert_eq!(state, JobNodeState::Confirmed, "{detail}");
    }
    // Planned again, nothing moves.
    let again = fleet.apply().await;
    assert_eq!(again.state, JobState::Succeeded, "{:?}", again.error);
    assert_eq!(step(&again, StepName::Channels).0, StepState::Skipped);
}

#[tokio::test]
async fn power_goes_with_the_fleet_and_channels_after() {
    let fleet = applied(&[AP1]).await;
    plan(&fleet, 6, TxPower::Dbm(17)).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(step(&job, StepName::Apply).0, StepState::Done);
    assert_eq!(step(&job, StepName::Channels).0, StepState::Done);
    let fake = fleet.fake(1);
    let tool = fake.tool.lock().unwrap();
    // The first transaction reloads (power alone keeps hostapd running), the second is live.
    assert_eq!(tool.live, [["radio1"]]);
    let wireless = tool.files["wireless"].clone();
    assert!(wireless.contains("option channel '6'") && wireless.contains("option txpower '17'"));
}

#[tokio::test]
async fn a_switch_that_is_refused_or_stuck_falls_back_to_a_reload() {
    for answer in [Switching::Refused, Switching::Stuck] {
        let fleet = applied(&[AP1]).await;
        fleet.fake(1).radios.lock().unwrap().answer = answer;
        plan(&fleet, 6, TxPower::Auto).await;
        let job = fleet.apply().await;
        assert_eq!(
            job.state,
            JobState::Succeeded,
            "{answer:?}: {:?}",
            job.error
        );
        let radios = fleet.fake(1).radios.lock().unwrap();
        assert_eq!(radios.reloads, 1, "{answer:?}");
        let updates = &job.nodes[1];
        assert_eq!(updates.state, JobNodeState::Confirmed);
        let applied = job
            .nodes
            .iter()
            .find(|n| n.node == fleet.nodes[1].0.id)
            .unwrap();
        assert_eq!(applied.state, JobNodeState::Confirmed);
        assert!(
            fleet
                .fake(1)
                .file("wireless")
                .unwrap()
                .contains("option channel '6'"),
            "{answer:?}"
        );
    }
}

#[tokio::test]
async fn a_failed_move_rolls_that_node_back_and_restarts_its_radios() {
    let fleet = applied(&[AP1, AP2]).await;
    let held = fleet.fake(1).file("wireless").unwrap();
    *fleet.fake(1).fault.lock().unwrap() = Some(Fault::Late);
    plan(&fleet, 6, TxPower::Auto).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.as_ref().unwrap();
    assert_eq!(error.code, JobErrorCode::PartialApply, "{}", error.message);
    assert!(
        error
            .message
            .starts_with("Moving the radios' channels failed"),
        "{}",
        error.message
    );
    assert_eq!(step(&job, StepName::Channels).0, StepState::Failed);
    let tool = fleet.fake(1).tool.lock().unwrap();
    assert_eq!(tool.restarted, [["radio1"]]);
    assert_eq!(tool.files["wireless"], held);
    // The next access point never started.
    assert_eq!(fleet.fake(2).tool.lock().unwrap().live.len(), 0);
}

#[tokio::test]
async fn a_mesh_radio_moves_with_the_fleet() {
    let fleet = applied(&[AP1]).await;
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.radio.plan.radios = vec![PlannedRadio {
        node: fleet.nodes[1].0.id,
        radio: "radio0".to_owned(),
        band: Band::G5,
        channel: 149,
        width: ChannelWidth::Mhz80,
        tx_power: TxPower::Auto,
    }];
    commit(&fleet.store, Some(head.meta.id), site).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(step(&job, StepName::Apply).0, StepState::Done);
    assert_eq!(step(&job, StepName::Channels).0, StepState::Skipped);
    assert_eq!(fleet.fake(1).tool.lock().unwrap().live.len(), 0);
    assert!(states(&job).contains(&(StepName::Channels, StepState::Skipped)));
}

/// Restarts the radios catching up on `fleet`'s access point and waits for the job to end.
async fn restart(fleet: &Fleet) -> Job {
    use uniwrt_store::JobRepo as _;
    let started = fleet
        .jobs
        .restart_radios(&[fleet.nodes[1].0.id])
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let job = fleet.store.job(started.id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the restart ends")
}

#[tokio::test]
async fn a_switched_radio_catches_up_in_the_restart_job() {
    let fleet = applied(&[AP1]).await;
    plan(&fleet, 6, TxPower::Auto).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(fleet.fake(1).radios.lock().unwrap().reloads, 0);

    let job = restart(&fleet).await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert_eq!(fleet.fake(1).radios.lock().unwrap().reloads, 1);
    let (state, detail) = node_detail(&job, 0);
    assert_eq!(
        (state, detail.as_str()),
        (JobNodeState::Verified, "Restarted radio1 on channel 6")
    );
    // Caught up: another restart leaves it alone.
    let again = restart(&fleet).await;
    assert_eq!(again.nodes[0].state, JobNodeState::Unchanged);
    assert_eq!(fleet.fake(1).radios.lock().unwrap().reloads, 1);
}

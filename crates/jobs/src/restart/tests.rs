//! The radio restart over the scripted node of [`crate::testing`], adopted and managed, with a
//! 5 GHz radio moved by a channel switch from 36 to 44.

use std::time::Duration;

use serde_json::{Value, json};
use uniwrt_core::NodeId;
use uniwrt_model::job::{Job, JobErrorCode, JobKind, JobNodeState, JobState, StepName, StepState};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::JobRepo as _;

use crate::{
    ApplyTiming, Error,
    testing::{COLLECTED, PASSWORD, Setup, setup, setup_timed, states},
};

/// The capture with radio1 on channel 44, which netifd still runs as 36 while `pending`.
fn collected(pending: bool) -> String {
    let mut value: Value = serde_json::from_str(COLLECTED).unwrap();
    let running = if pending { "36" } else { "44" };
    value["sets"]["radios"] = json!({
        "wireless": {"radio1": {
            "up": true,
            "config": {"band": "5g", "channel": running, "htmode": "HE80"},
            "saved": {"channel": "44", "htmode": "HE80"},
            "interfaces": [{"section": "default_radio1", "ifname": "phy1-ap0",
                            "config": {"mode": "ap", "ssid": "home"}}],
        }},
        "interfaces": [{"ifname": "phy1-ap0", "wiphy": 1, "wiphy_freq": 5220,
                        "channel_width": 3, "center_freq1": 5210}],
    });
    value.to_string()
}

fn ok() -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

async fn finished(setup: &Setup, job: Job) -> Job {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = setup.store.job(job.id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the job ends")
}

async fn managed(setup: &Setup, manage: bool) -> NodeId {
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let id = job.node.unwrap();
    if manage {
        let started = setup.jobs.manage(id, true, None).await.unwrap();
        assert_eq!(finished(setup, started).await.state, JobState::Succeeded);
    }
    id
}

/// The node's radio is caught up by `wifi reload` when `restarts`.
fn play(setup: &Setup, pending: bool, restarts: bool) {
    *setup.node.collected.lock().unwrap() = Some(collected(pending));
    let node = std::sync::Arc::clone(&setup.node);
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv: &[&str]| match argv {
        ["wifi", "reload"] => {
            if restarts {
                *node.collected.lock().unwrap() = Some(collected(false));
            }
            Some(Ok(ok()))
        }
        _ => None,
    }));
}

fn reloaded(setup: &Setup) -> bool {
    let commands = setup.node.commands.lock().unwrap();
    commands.iter().any(|argv| argv == &["wifi", "reload"])
}

async fn restart(setup: &Setup, id: NodeId) -> Job {
    let started = setup.jobs.restart_radios(&[id]).await.unwrap();
    assert_eq!(started.kind, JobKind::RadioRestart);
    finished(setup, started).await
}

#[tokio::test]
async fn a_radio_catching_up_restarts_until_netifd_runs_its_saved_channel() {
    let setup = setup(true).await;
    let id = managed(&setup, true).await;
    play(&setup, true, true);
    let job = restart(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(states(&job), [(StepName::Restart, StepState::Done)]);
    assert!(reloaded(&setup));
    let node = &job.nodes[0];
    assert_eq!(node.state, JobNodeState::Verified);
    assert_eq!(
        node.detail.as_deref(),
        Some("Restarted radio1 on channel 44")
    );
    assert_eq!(
        job.steps[0].detail.as_deref(),
        Some("Restarted the radios of 1 node, one at a time")
    );
}

#[tokio::test]
async fn a_node_caught_up_meanwhile_is_left_alone() {
    let setup = setup(true).await;
    let id = managed(&setup, true).await;
    play(&setup, false, true);
    let job = restart(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(job.nodes[0].state, JobNodeState::Unchanged);
    assert!(!reloaded(&setup));
}

#[tokio::test]
async fn a_radio_that_does_not_come_back_fails_the_job() {
    let timing = ApplyTiming {
        poll: Duration::from_millis(5),
        verify_uplink: Duration::from_millis(50),
        ..ApplyTiming::default()
    };
    let setup = setup_timed(true, timing).await;
    let id = managed(&setup, true).await;
    play(&setup, true, false);
    let job = restart(&setup, id).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error
            .message
            .contains("radio1 didn't come back on the saved channel"),
        "{}",
        error.message
    );
    assert_eq!(job.nodes[0].state, JobNodeState::Failed);
}

#[tokio::test]
async fn nodes_in_observe_mode_are_refused() {
    let setup = setup(true).await;
    let id = managed(&setup, false).await;
    assert!(matches!(
        setup.jobs.restart_radios(&[id]).await,
        Err(Error::Observed(_))
    ));
    assert!(matches!(
        setup.jobs.restart_radios(&[]).await,
        Err(Error::Failed(_))
    ));
}

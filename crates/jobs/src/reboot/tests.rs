//! Restarts over the scripted node of [`crate::testing`], adopted and managed: `reboot` takes it
//! down for a few tries of reaching it, or not at all.

use std::{sync::atomic::Ordering, time::Duration};

use uniwrt_core::NodeId;
use uniwrt_model::job::{Job, JobErrorCode, JobKind, JobNodeState, JobState, StepName, StepState};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::JobRepo as _;

use crate::{
    ApplyTiming, Error, RebootTiming,
    testing::{PASSWORD, Setup, setup, setup_timed, states},
};

fn quick() -> ApplyTiming {
    ApplyTiming {
        reboot: RebootTiming {
            poll: Duration::from_millis(5),
            down: Duration::from_millis(50),
            back: Duration::from_millis(200),
        },
        ..ApplyTiming::default()
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

/// `reboot` answers `code` and takes the node down for `down` tries of reaching it.
fn play(setup: &Setup, code: u32, down: u32) {
    let node = std::sync::Arc::clone(&setup.node);
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv: &[&str]| match argv {
        ["reboot"] => {
            node.down.store(down, Ordering::SeqCst);
            Some(Ok(ExecOutput {
                status: ExitStatus::Code(code),
                stdout: Vec::new(),
                stderr: if code == 0 {
                    Vec::new()
                } else {
                    b"reboot: Operation not permitted\n".to_vec()
                },
            }))
        }
        _ => None,
    }));
}

fn rebooted(setup: &Setup) -> bool {
    let commands = setup.node.commands.lock().unwrap();
    commands.iter().any(|argv| argv == &["reboot"])
}

async fn reboot(setup: &Setup, id: NodeId) -> Job {
    let started = setup.jobs.reboot(&[id, id], None).await.unwrap();
    assert_eq!(started.kind, JobKind::Reboot);
    assert_eq!(started.nodes.len(), 1, "a node named twice restarts once");
    finished(setup, started).await
}

#[tokio::test]
async fn a_node_goes_down_and_comes_back() {
    let setup = setup_timed(true, quick()).await;
    let id = managed(&setup, true).await;
    play(&setup, 0, 3);
    let job = reboot(&setup, id).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(states(&job), [(StepName::Reboot, StepState::Done)]);
    assert!(rebooted(&setup));
    let node = &job.nodes[0];
    assert_eq!(node.state, JobNodeState::Verified);
    assert!(
        node.detail.as_deref().unwrap().starts_with("Back after"),
        "{node:?}"
    );
    assert_eq!(job.target, job.nodes[0].name);
    assert_eq!(
        job.steps[0].detail.as_deref(),
        Some(format!("{} restarted", job.target).as_str())
    );
}

#[tokio::test]
async fn a_node_never_seen_down_must_have_booted_since() {
    let setup = setup_timed(true, quick()).await;
    let id = managed(&setup, true).await;
    // The capture's uptime is 401 min, far longer than the job took.
    play(&setup, 0, 0);
    let job = reboot(&setup, id).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NodeFailed);
    assert!(
        error.message.ends_with("It didn't restart: up for 401 min"),
        "{}",
        error.message
    );
    assert_eq!(job.nodes[0].state, JobNodeState::Failed);
}

#[tokio::test]
async fn a_node_that_does_not_come_back_fails_the_job() {
    let setup = setup_timed(true, quick()).await;
    let id = managed(&setup, true).await;
    play(&setup, 0, u32::MAX);
    let job = reboot(&setup, id).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::NotBack);
    assert!(
        error.message.contains("didn't come back within"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_refused_reboot_fails_at_once() {
    let setup = setup_timed(true, quick()).await;
    let id = managed(&setup, true).await;
    play(&setup, 1, 0);
    let job = reboot(&setup, id).await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let message = job.error.unwrap().message;
    assert!(
        message.ends_with("reboot refused: reboot: Operation not permitted"),
        "{message}"
    );
}

#[tokio::test]
async fn only_managed_nodes_restart() {
    let setup = setup(true).await;
    let id = managed(&setup, false).await;
    assert!(matches!(
        setup.jobs.reboot(&[id], None).await,
        Err(Error::Observed(_))
    ));
    assert!(matches!(
        setup.jobs.reboot(&[], None).await,
        Err(Error::Failed(_))
    ));
    assert!(matches!(
        setup.jobs.reboot(&[NodeId::new(99)], None).await,
        Err(Error::NoNode(_))
    ));
}

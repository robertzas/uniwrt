//! Management on and off over the scripted node of [`crate::testing`], once it is adopted.

use std::{sync::Arc, time::Duration};

use tokio::sync::Notify;
use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobErrorCode, JobKind, JobState, StepName, StepState},
    node::NodeMode,
};
use uniwrt_node::txn::Footprint;
use uniwrt_store::{JobRepo as _, NodeRepo as _};

use crate::{
    Error,
    testing::{PASSWORD, Setup, setup, states},
};

async fn adopted() -> (Setup, NodeId) {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let id = job.node.unwrap();
    (setup, id)
}

async fn finished(setup: &Setup, job: Job) -> Job {
    tokio::time::timeout(Duration::from_secs(5), async {
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

async fn switch(setup: &Setup, id: NodeId, enable: bool) -> Job {
    let started = setup.jobs.manage(id, enable, None).await.unwrap();
    finished(setup, started).await
}

async fn mode(setup: &Setup, id: NodeId) -> NodeMode {
    setup.store.node(id).await.unwrap().unwrap().mode
}

fn detail(job: &Job, step: StepName) -> String {
    job.steps
        .iter()
        .find(|s| s.name == step)
        .and_then(|s| s.detail.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn managing_installs_the_tool_and_unmanaging_removes_it() {
    let (setup, id) = adopted().await;
    assert_eq!(mode(&setup, id).await, NodeMode::Observe);

    let job = switch(&setup, id, true).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        (job.kind, job.node, job.target.as_str()),
        (JobKind::Manage, Some(id), "OpenWrt")
    );
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, StepState::Done),
            (StepName::InstallTool, StepState::Done),
            (StepName::Record, StepState::Done),
        ]
    );
    assert_eq!(mode(&setup, id).await, NodeMode::Managed);
    assert_eq!(
        setup.node.footprint.lock().unwrap().as_deref(),
        Some(Footprint::get().version())
    );

    // Again: the same version stays.
    let again = switch(&setup, id, true).await;
    assert_eq!(again.state, JobState::Succeeded);
    assert!(detail(&again, StepName::InstallTool).contains("already"));

    let off = switch(&setup, id, false).await;
    assert_eq!(off.state, JobState::Succeeded, "{off:#?}");
    assert_eq!(off.kind, JobKind::Unmanage);
    assert_eq!(states(&off)[1], (StepName::RemoveTool, StepState::Done));
    assert_eq!(mode(&setup, id).await, NodeMode::Observe);
    assert_eq!(*setup.node.footprint.lock().unwrap(), None);

    // A node without the tool goes to observe mode all the same.
    let bare = switch(&setup, id, false).await;
    assert_eq!(states(&bare)[1], (StepName::RemoveTool, StepState::Skipped));
}

#[tokio::test]
async fn an_open_transaction_keeps_the_tool_where_it_is() {
    let (setup, id) = adopted().await;
    assert_eq!(switch(&setup, id, true).await.state, JobState::Succeeded);
    *setup.node.open_txn.lock().unwrap() = Some("j9".to_owned());

    for enable in [false, true] {
        let job = switch(&setup, id, enable).await;
        assert_eq!(job.state, JobState::Failed);
        let error = job.error.unwrap();
        assert_eq!(error.code, JobErrorCode::NodeFailed);
        assert!(
            error.message.contains("Transaction j9 is open"),
            "{}",
            error.message
        );
    }
    assert_eq!(mode(&setup, id).await, NodeMode::Managed);
    assert!(setup.node.footprint.lock().unwrap().is_some());
}

#[tokio::test]
async fn one_job_at_a_time_per_node_and_only_for_known_nodes() {
    let (setup, id) = adopted().await;
    assert!(matches!(
        setup.jobs.manage(NodeId::new(99), true, None).await,
        Err(Error::NoNode(_))
    ));

    let gate = Arc::new(Notify::new());
    *setup.node.gate.lock().unwrap() = Some(Arc::clone(&gate));
    let first = setup.jobs.manage(id, true, None).await.unwrap();
    assert!(matches!(
        setup.jobs.manage(id, false, None).await,
        Err(Error::Busy { .. })
    ));
    *setup.node.gate.lock().unwrap() = None;
    gate.notify_one();
    assert_eq!(finished(&setup, first).await.state, JobState::Succeeded);
    assert_eq!(switch(&setup, id, false).await.state, JobState::Succeeded);
}

#[tokio::test]
async fn without_a_dial_nothing_is_managed() {
    let setup = setup(false).await;
    assert!(matches!(
        setup.jobs.manage(NodeId::new(1), true, None).await,
        Err(Error::Unavailable)
    ));
}

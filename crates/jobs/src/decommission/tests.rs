//! Decommissioning and forgetting over the scripted node of [`crate::testing`].

use std::time::Duration;

use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobKind, JobState, StepName, StepState},
    node::{NewNode, NodeMode, NodeRole},
    site::{
        SiteConfig,
        node::NodeConfig,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::{
    HostKeyRepo as _, JobRepo as _, NodeRepo as _, RevisionRepo as _, UserRepo as _,
};

use super::Author;
use crate::{
    Error,
    testing::{PASSWORD, Setup, setup, states},
};

async fn adopted() -> (Setup, NodeId) {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    (setup, job.node.unwrap())
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

/// A revision whose document has overrides for `node`.
async fn naming(setup: &Setup, node: NodeId) {
    let mut config = SiteConfig::defaults(None);
    config.nodes.push(NodeConfig::new(node));
    setup
        .store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: None,
                author: None,
                message: "overrides".to_owned(),
                config,
            },
            Vec::new(),
        )
        .await
        .unwrap();
}

async fn admin(setup: &Setup) -> Author {
    let user = setup
        .store
        .create_first_user("admin", uniwrt_model::user::Role::Admin, "$argon2id$x")
        .await
        .unwrap();
    Author {
        id: user.id,
        name: user.username,
    }
}

fn wipes(setup: &Setup) -> Vec<Vec<String>> {
    setup
        .node
        .commands
        .lock()
        .unwrap()
        .iter()
        .filter(|argv| argv.get(5).map(String::as_str) == Some(uniwrt_node::decommission::LOG))
        .cloned()
        .collect()
}

#[tokio::test]
async fn a_decommission_wipes_the_node_then_the_controller_forgets_it() {
    let (setup, id) = adopted().await;
    let job = setup.jobs.manage(id, true, None).await.unwrap();
    assert_eq!(finished(&setup, job).await.state, JobState::Succeeded);
    naming(&setup, id).await;
    let key = setup.key.authorized_key().unwrap();
    assert!(
        setup
            .node
            .authorized_keys
            .lock()
            .unwrap()
            .contains(key.trim())
    );

    let started = setup
        .jobs
        .decommission(id, false, Some(admin(&setup).await))
        .await
        .unwrap();
    let job = finished(&setup, started).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!((job.kind, job.node), (JobKind::Decommission, Some(id)));
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, StepState::Done),
            (StepName::RemoveTool, StepState::Done),
            (StepName::Wipe, StepState::Done),
            (StepName::Record, StepState::Done),
        ]
    );
    assert_eq!(*setup.node.footprint.lock().unwrap(), None);
    let wiped = wipes(&setup);
    assert_eq!(wiped.len(), 1);
    assert_eq!(wiped[0][6..], [key.trim(), "0"]);
    assert!(
        !setup
            .node
            .authorized_keys
            .lock()
            .unwrap()
            .contains(key.trim())
    );
    assert!(setup.watched.2.lock().unwrap().contains(&id));

    assert_eq!(setup.store.node(id).await.unwrap(), None);
    assert_eq!(setup.store.host_key(id).await.unwrap(), None);
    let head = setup.store.head_revision().await.unwrap().unwrap();
    assert_eq!(head.meta.kind, RevisionKind::Remove);
    assert_eq!(head.meta.author.as_deref(), Some("admin"));
    assert_eq!(head.config.nodes, []);
    assert!(
        job.steps[3]
            .detail
            .as_deref()
            .unwrap()
            .contains(&format!("revision {}", head.meta.id)),
        "{job:#?}"
    );
}

#[tokio::test]
async fn firstboot_is_asked_of_the_wipe() {
    let (setup, id) = adopted().await;
    let job = setup.jobs.decommission(id, true, None).await.unwrap();
    let job = finished(&setup, job).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(states(&job)[1], (StepName::RemoveTool, StepState::Skipped));
    assert_eq!(wipes(&setup)[0][7], "1");
    // No revision named the node, so none is saved.
    assert_eq!(setup.store.head_revision().await.unwrap(), None);
}

#[tokio::test]
async fn a_wipe_that_fails_leaves_the_node_watched_and_recorded() {
    let (setup, id) = adopted().await;
    let job = setup.jobs.manage(id, true, None).await.unwrap();
    assert_eq!(finished(&setup, job).await.state, JobState::Succeeded);
    *setup.node.script.lock().unwrap() = Some(Box::new(|argv| {
        (argv.get(5) == Some(&uniwrt_node::decommission::LOG)).then(|| {
            Ok(ExecOutput {
                status: ExitStatus::Code(1),
                stdout: Vec::new(),
                stderr: b"sh: out of memory".to_vec(),
            })
        })
    }));

    let job = setup.jobs.decommission(id, false, None).await.unwrap();
    let job = finished(&setup, job).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(states(&job)[2], (StepName::Wipe, StepState::Failed));
    let node = setup.store.node(id).await.unwrap().unwrap();
    // The tool went, so the node is observed from then on.
    assert_eq!(node.mode, NodeMode::Observe);
    let moved = setup.watched.1.lock().unwrap();
    assert_eq!(moved.iter().map(|n| n.id).collect::<Vec<_>>(), [id]);
}

#[tokio::test]
async fn an_open_transaction_stops_it_before_anything_changes() {
    let (setup, id) = adopted().await;
    let job = setup.jobs.manage(id, true, None).await.unwrap();
    assert_eq!(finished(&setup, job).await.state, JobState::Succeeded);
    *setup.node.open_txn.lock().unwrap() = Some("j9".to_owned());

    let job = setup.jobs.decommission(id, false, None).await.unwrap();
    let job = finished(&setup, job).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(states(&job)[1], (StepName::RemoveTool, StepState::Failed));
    assert_eq!(
        setup.store.node(id).await.unwrap().unwrap().mode,
        NodeMode::Managed
    );
    assert_eq!(wipes(&setup), [] as [Vec<String>; 0]);
    assert_eq!(*setup.watched.2.lock().unwrap(), []);
}

#[tokio::test]
async fn forgetting_logs_in_nowhere() {
    let (setup, id) = adopted().await;
    naming(&setup, id).await;
    let before = setup.node.commands.lock().unwrap().len();

    let job = setup
        .jobs
        .forget(id, Some(admin(&setup).await))
        .await
        .unwrap();
    let job = finished(&setup, job).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(states(&job), [(StepName::Record, StepState::Done)]);
    assert_eq!(setup.node.commands.lock().unwrap().len(), before);
    assert_eq!(setup.store.node(id).await.unwrap(), None);
    assert_eq!(
        setup
            .store
            .head_revision()
            .await
            .unwrap()
            .unwrap()
            .meta
            .kind,
        RevisionKind::Remove
    );
}

#[tokio::test]
async fn the_gateway_goes_last() {
    let (setup, id) = adopted().await;
    let gateway = setup
        .store
        .create_node(NewNode {
            name: "gateway".to_owned(),
            mac: "94:83:c4:00:00:01".parse().unwrap(),
            board_name: "glinet,gl-mt6000".to_owned(),
            host: "10.0.0.1".to_owned(),
            port: 22,
            role: NodeRole::Gateway,
        })
        .await
        .unwrap();
    for refused in [
        setup.jobs.forget(gateway.id, None).await,
        setup.jobs.decommission(gateway.id, true, None).await,
    ] {
        assert!(
            matches!(&refused, Err(Error::Refused(why)) if why.contains("gateway")),
            "{refused:?}"
        );
    }
    let job = setup.jobs.forget(id, None).await.unwrap();
    assert_eq!(finished(&setup, job).await.state, JobState::Succeeded);
    let job = setup.jobs.forget(gateway.id, None).await.unwrap();
    assert_eq!(finished(&setup, job).await.state, JobState::Succeeded);
    assert_eq!(setup.store.nodes().await.unwrap(), []);
    assert!(matches!(
        setup.jobs.forget(gateway.id, None).await,
        Err(Error::NoNode(_))
    ));
}

//! Re-trusting a node's changed host key (D-154), over the scripted node of [`crate::testing`]:
//! a device at the node's address that presents a new key.

use std::{sync::Arc, time::Duration};

use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobErrorCode, JobKind, JobState, StepName, StepState},
    node::NodeMode,
};
use uniwrt_node::HostKey;
use uniwrt_store::{HostKeyRepo as _, JobRepo as _, NodeRepo as _};
use zeroize::Zeroizing;

use crate::{
    Retrust,
    testing::{COLLECTED, FakeNode, PASSWORD, Setup, setup, states},
};

/// The adopted node, managed.
async fn managed() -> (Setup, NodeId) {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let id = job.node.unwrap();
    setup
        .store
        .set_node_mode(id, NodeMode::Managed)
        .await
        .unwrap();
    (setup, id)
}

/// The device answering at the node's address from now on, with a new host key; it still
/// trusts the controller's key when `keeps_key`, and root's password is `password`.
fn reflashed(setup: &Setup, keeps_key: bool, password: &str) -> Arc<FakeNode> {
    let device = FakeNode::factory();
    if keeps_key {
        *device.authorized_keys.lock().unwrap() =
            setup.node.authorized_keys.lock().unwrap().clone();
    }
    *device.password.lock().unwrap() = password.to_owned();
    setup
        .node
        .hosts
        .lock()
        .unwrap()
        .insert("10.0.0.2".to_owned(), Arc::clone(&device));
    device
}

fn retrust(key: &HostKey, password: Option<&str>) -> Retrust {
    Retrust {
        host_key: key.clone(),
        password: password.map(|password| Zeroizing::new(password.to_owned())),
        remember_password: false,
        allow_unsupported_release: false,
    }
}

async fn run(setup: &Setup, id: NodeId, retrust: Retrust) -> Job {
    let started = setup.jobs.retrust(id, retrust, None).await.unwrap();
    assert_eq!(
        (started.kind, started.node, started.state),
        (JobKind::Retrust, Some(id), JobState::Running)
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let job = setup.store.job(started.id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the job ends")
}

async fn pinned(setup: &Setup, id: NodeId) -> String {
    setup.store.host_key(id).await.unwrap().unwrap().fingerprint
}

#[tokio::test]
async fn a_device_that_still_trusts_the_key_gets_its_new_key_pinned_and_stays_as_it_was() {
    use StepState::{Done, Skipped};

    let (setup, id) = managed().await;
    let device = reflashed(&setup, true, PASSWORD);
    let keys = device.authorized_keys.lock().unwrap().clone();

    let job = run(&setup, id, retrust(&device.host_key, None)).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, Done),
            (StepName::Check, Done),
            (StepName::InstallKey, Skipped),
            (StepName::VerifyKey, Skipped),
            (StepName::SetPassword, Skipped),
            (StepName::Inventory, Skipped),
            (StepName::Backup, Skipped),
            (StepName::Record, Done),
            (StepName::Watch, Done),
        ]
    );
    let record = job.steps.iter().find(|step| step.name == StepName::Record);
    let detail = record.and_then(|step| step.detail.as_deref()).unwrap();
    assert!(detail.contains(device.host_key.fingerprint()), "{detail}");
    assert!(detail.ends_with("stays managed"), "{detail}");

    assert_eq!(pinned(&setup, id).await, device.host_key.fingerprint());
    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mode, NodeMode::Managed);
    assert_eq!(
        setup.watched.1.lock().unwrap().last().map(|n| n.id),
        Some(id)
    );
    // Nothing changed on the device.
    assert_eq!(*device.authorized_keys.lock().unwrap(), keys);
    assert!(!device.commands().iter().any(|command| command == "passwd"));

    // The pinned key trusted again reconnects, as when the device's old key came back.
    let again = run(&setup, id, retrust(&device.host_key, None)).await;
    assert_eq!(again.state, JobState::Succeeded, "{again:#?}");
    assert_eq!(setup.watched.1.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_reset_device_asks_for_its_password_then_is_adopted_back_in_observe_mode() {
    let (setup, id) = managed().await;
    let device = reflashed(&setup, false, "the new root password");

    // Neither the controller's key nor an empty password gets in.
    let refused = run(&setup, id, retrust(&device.host_key, None)).await;
    assert_eq!(refused.state, JobState::Failed);
    assert_eq!(
        refused.error.map(|error| error.code),
        Some(JobErrorCode::AuthRejected)
    );
    assert_ne!(pinned(&setup, id).await, device.host_key.fingerprint());

    let job = run(
        &setup,
        id,
        retrust(&device.host_key, Some("the new root password")),
    )
    .await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let done = |name| {
        job.steps
            .iter()
            .any(|step| step.name == name && step.state == StepState::Done)
    };
    assert!(
        done(StepName::InstallKey) && done(StepName::Backup),
        "{job:#?}"
    );
    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!((node.id, node.mode), (id, NodeMode::Observe));
    assert_eq!(pinned(&setup, id).await, device.host_key.fingerprint());
    let key = setup.key.authorized_key().unwrap();
    assert!(device.authorized_keys.lock().unwrap().contains(&key));
}

#[tokio::test]
async fn a_factory_device_is_adopted_back_without_a_password() {
    let (setup, id) = managed().await;
    let device = reflashed(&setup, false, "");

    let job = run(&setup, id, retrust(&device.host_key, None)).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let password = job
        .steps
        .iter()
        .find(|step| step.name == StepName::SetPassword);
    assert_eq!(password.map(|step| step.state), Some(StepState::Done));
    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mode, NodeMode::Observe);
}

#[tokio::test]
async fn another_device_at_the_address_is_refused_and_the_pin_stays() {
    let (setup, id) = managed().await;
    let before = pinned(&setup, id).await;
    let device = reflashed(&setup, true, PASSWORD);
    *device.collected.lock().unwrap() =
        Some(COLLECTED.replace("1e:09:3b:df:16:4c", "1e:09:3b:df:16:99"));

    let job = run(&setup, id, retrust(&device.host_key, None)).await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::IdentityMismatch);
    assert!(
        error.message.contains("Replace its device instead"),
        "{}",
        error.message
    );
    assert_eq!(pinned(&setup, id).await, before);
    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mode, NodeMode::Managed);
}

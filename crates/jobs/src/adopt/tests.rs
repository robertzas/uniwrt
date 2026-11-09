//! Adoptions over the scripted node of [`crate::testing`] and a temporary store.

use std::{sync::Arc, time::Duration};

use tokio::sync::{Notify, broadcast};
use uniwrt_model::{
    job::{Job, JobErrorCode, JobState, StepName, StepState},
    node::{NodeMode, NodeRole},
};
use uniwrt_node::{ControllerKey, HostKey, Target};
use uniwrt_store::{
    HostKeyRepo as _, InventoryRepo as _, JobRepo as _, NodeRepo as _, VaultRepo as _,
};

use crate::Error;

use crate::testing::{PASSWORD, TARBALL, setup, states};

#[tokio::test]
async fn adopting_with_the_password_installs_the_key_records_everything_and_watches() {
    let setup = setup(true).await;
    let mut request = setup.request(Some(PASSWORD));
    request.remember_password = true;
    let (job, seen) = setup.adopt(request).await;

    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    for step in &job.steps {
        let expected = if step.name == StepName::SetPassword {
            StepState::Skipped
        } else {
            StepState::Done
        };
        assert_eq!(step.state, expected, "{step:?}");
    }
    assert!(job.finished_at.is_some());
    assert!(
        seen.len() > 2 * job.steps.len(),
        "every step's start and end"
    );
    let id = job.node.unwrap();

    // Only the controller's key changed on the node.
    assert_eq!(
        *setup.node.authorized_keys.lock().unwrap(),
        format!("{}\n", setup.key.authorized_key().unwrap())
    );
    assert_eq!(
        setup.node.commands(),
        ["cat", "sh", "grep", "sysupgrade"],
        "read the release, install the key, read the packages, back up"
    );

    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mode, NodeMode::Observe);
    assert_eq!(node.role, NodeRole::Ap);
    assert_eq!((node.host.as_str(), node.port), ("10.0.0.2", 22));
    assert_eq!(node.name, "OpenWrt");
    assert_eq!(node.board_name, "container");
    let pin = setup.store.host_key(id).await.unwrap().unwrap();
    assert_eq!(pin.fingerprint, setup.node.host_key.fingerprint());
    let inventory = setup.store.inventory(id).await.unwrap().unwrap();
    let names: Vec<_> = inventory
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package.explicit))
        .collect();
    assert_eq!(names, [("dropbear", false), ("luci", true)]);
    let backups = setup.store.backups(id).await.unwrap();
    let (_, tarball) = setup
        .vault
        .open_backup(backups[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tarball.as_slice(), TARBALL);
    let password = setup
        .vault
        .secret(&format!("node/{id}/root_password"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(password.as_slice(), PASSWORD.as_bytes());

    let watched = setup.watched.0.lock().unwrap();
    assert_eq!(watched.len(), 1);
    assert_eq!(watched[0].0, node);
    assert_eq!(watched[0].1, setup.node.host_key);
    assert_eq!(watched[0].2.board.board_name, "container");
}

#[tokio::test]
async fn a_node_that_trusts_the_key_needs_no_password_and_no_change() {
    let setup = setup(true).await;
    setup
        .node
        .authorized_keys
        .lock()
        .unwrap()
        .push_str(&(setup.key.authorized_key().unwrap() + "\n"));
    let mut request = setup.request(None);
    request.name = Some("  attic  ".to_owned());
    request.role = Some(NodeRole::Bridge);
    let (job, _) = setup.adopt(request).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let skipped: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.state == StepState::Skipped)
        .map(|step| step.name)
        .collect();
    assert_eq!(
        skipped,
        [
            StepName::InstallKey,
            StepName::VerifyKey,
            StepName::SetPassword
        ]
    );
    assert!(!setup.node.commands().contains(&"sh".to_owned()));
    let node = setup.store.node(job.node.unwrap()).await.unwrap().unwrap();
    assert_eq!((node.name.as_str(), node.role), ("attic", NodeRole::Bridge));
    assert!(
        setup
            .vault
            .secret(&format!("node/{}/root_password", node.id))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_node_without_a_root_password_gets_a_random_one_kept_in_the_vault() {
    for given in [Some(""), None] {
        let setup = setup(true).await;
        setup.node.password.lock().unwrap().clear();
        let (job, _) = setup.adopt(setup.request(given)).await;
        assert_eq!(job.state, JobState::Succeeded, "{given:?}: {job:#?}");
        assert!(
            job.steps.iter().all(|step| step.state == StepState::Done),
            "{job:#?}"
        );
        let connect = job.steps.iter().find(|s| s.name == StepName::Connect);
        assert!(
            connect
                .and_then(|step| step.detail.as_deref())
                .is_some_and(|detail| detail.contains("no password")),
            "{connect:?}"
        );
        assert_eq!(
            setup.node.commands(),
            ["cat", "sh", "passwd", "grep", "sysupgrade"],
            "the password is set after the key works, before the inventory"
        );
        let set = setup.node.password.lock().unwrap().clone();
        assert_eq!(set.len(), 29, "{set}");
        let id = job.node.unwrap();
        let kept = setup
            .vault
            .secret(&format!("node/{id}/root_password"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.as_slice(), set.as_bytes());
        let node = setup.store.node(id).await.unwrap().unwrap();
        assert!(
            setup
                .vault
                .secret(&format!("adopt/{}/root_password", node.mac))
                .await
                .unwrap()
                .is_none(),
            "moved to the node's name"
        );
    }
}

#[tokio::test]
async fn a_root_password_kept_by_a_failed_adoption_moves_to_the_node_on_the_next() {
    let setup = setup(true).await;
    // An earlier adoption set the password and installed the key, then failed to record.
    setup
        .node
        .authorized_keys
        .lock()
        .unwrap()
        .push_str(&(setup.key.authorized_key().unwrap() + "\n"));
    let mac = "1e:09:3b:df:16:4c";
    setup
        .vault
        .put_secret(
            &format!("adopt/{mac}/root_password"),
            zeroize::Zeroizing::new(b"kept-earlier".to_vec()),
        )
        .await
        .unwrap();
    let (job, _) = setup.adopt(setup.request(None)).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let id = job.node.unwrap();
    let node = setup.store.node(id).await.unwrap().unwrap();
    assert_eq!(node.mac.to_string(), mac, "the capture's MAC");
    let kept = setup
        .vault
        .secret(&format!("node/{id}/root_password"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.as_slice(), b"kept-earlier");
}

#[tokio::test]
async fn a_wrong_password_fails_at_connect_and_changes_nothing() {
    let setup = setup(true).await;
    let (job, _) = setup.adopt(setup.request(Some("wrong"))).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.as_ref().unwrap().code, JobErrorCode::AuthRejected);
    assert_eq!(
        states(&job)[..2],
        [
            (StepName::Connect, StepState::Failed),
            (StepName::Check, StepState::Pending)
        ]
    );
    assert_eq!(setup.node.commands(), [""; 0]);
    assert_eq!(setup.store.nodes().await.unwrap(), []);
    assert!(setup.watched.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn another_host_key_than_the_confirmed_one_is_refused() {
    let setup = setup(true).await;
    let mut request = setup.request(Some(PASSWORD));
    request.host_key =
        HostKey::parse(&ControllerKey::generate().unwrap().authorized_key().unwrap()).unwrap();
    let (job, _) = setup.adopt(request).await;
    assert_eq!(job.error.unwrap().code, JobErrorCode::HostKeyMismatch);
    assert_eq!(setup.node.commands(), [""; 0]);
}

#[tokio::test]
async fn an_old_release_fails_the_check_before_any_change() {
    let setup = setup(true).await;
    *setup.node.release.lock().unwrap() =
        "DISTRIB_ID='OpenWrt'\nDISTRIB_RELEASE='23.05.5'\n".to_owned();
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::Unsupported);
    assert!(error.message.contains("23.05.5"), "{error:?}");
    assert_eq!(states(&job)[1], (StepName::Check, StepState::Failed));
    assert_eq!(setup.node.commands(), ["cat"]);
    assert!(setup.node.authorized_keys.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_adopted_already_fails_the_check() {
    let setup = setup(true).await;
    let (first, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(first.state, JobState::Succeeded);
    let (again, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    let error = again.error.unwrap();
    assert_eq!(error.code, JobErrorCode::AlreadyAdopted);
    assert_eq!(error.message, "The node is adopted already, as OpenWrt");
    assert_eq!(setup.store.nodes().await.unwrap().len(), 1);
    assert_eq!(setup.jobs.jobs(10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn one_job_at_a_time_per_target_and_a_stop_interrupts_it() {
    let setup = setup(true).await;
    let gate = Arc::new(Notify::new());
    *setup.node.gate.lock().unwrap() = Some(Arc::clone(&gate));
    let mut updates = setup.jobs.subscribe();
    let job = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap();
    // The job is connecting, held at the gate.
    wait_for(&mut updates, |job| job.steps[0].state == StepState::Running).await;
    let err = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Busy { .. }), "{err:?}");

    setup.jobs.shutdown().await;
    let stopped = setup.store.job(job.id).await.unwrap().unwrap();
    assert_eq!(stopped.state, JobState::Failed);
    assert_eq!(stopped.error.unwrap().code, JobErrorCode::Interrupted);
    assert_eq!(stopped.steps[0].state, StepState::Failed);
    assert_eq!(setup.store.nodes().await.unwrap(), []);
}

#[tokio::test]
async fn a_finished_job_frees_its_target() {
    let setup = setup(true).await;
    let (first, _) = setup.adopt(setup.request(Some("wrong"))).await;
    assert_eq!(first.state, JobState::Failed);
    let (second, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(second.state, JobState::Succeeded);
}

#[tokio::test]
async fn without_a_dial_nothing_can_be_adopted() {
    let setup = setup(false).await;
    assert!(!setup.jobs.can_adopt());
    let err = setup
        .jobs
        .adopt(setup.request(Some(PASSWORD)), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unavailable));
    let target = Target::new("10.0.0.2", 22);
    assert!(matches!(
        setup.jobs.probe(&target).await.unwrap_err(),
        Error::Unavailable
    ));
    assert_eq!(setup.jobs.jobs(10).await.unwrap(), []);
}

async fn wait_for(updates: &mut broadcast::Receiver<Arc<Job>>, done: impl Fn(&Job) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !done(&updates.recv().await.unwrap()) {}
    })
    .await
    .expect("the update comes");
}

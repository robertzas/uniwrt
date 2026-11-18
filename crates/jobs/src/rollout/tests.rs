//! Rollouts over the scripted node of [`crate::testing`]: the adopted node and two more records
//! pinned to the same device, so each upgrade job flashes it again (c1's script), with images
//! from a scripted [`RolloutImages`].

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use uniwrt_core::{Clock as _, MacAddr, NodeId};
use uniwrt_model::{
    job::{JobError, JobErrorCode, JobKind, JobState, NewJob},
    node::{NewNode, NodeMode, NodeRole},
    rollout::{
        NewRollout, Rollout, RolloutNode, RolloutNodeState, RolloutState, RolloutWait,
        UpgradeChoice, WINDOW,
    },
};
use uniwrt_store::{HostKeyRepo as _, JobRepo as _, NodeRepo as _, RolloutRepo as _};

use super::{CanaryRequest, RolloutImages, RolloutRequest, RolloutTiming, Rollouts};
use crate::{
    BoxFuture, Error, UpgradeRequest,
    testing::{Setup, setup_timed},
    upgrade::tests::{Fetch, GOOD, IMAGE, managed, play, request, timing},
};

/// Images for the scripted device: the release it runs now to 25.12.6; `failing` nodes' fail.
struct Images {
    setup_node: Arc<crate::testing::FakeNode>,
    image: PathBuf,
    ahead: Mutex<Vec<NodeId>>,
    asked: Mutex<Vec<NodeId>>,
    failing: Mutex<BTreeSet<NodeId>>,
}

impl RolloutImages for Images {
    fn ahead<'a>(
        &'a self,
        node: NodeId,
        _choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.ahead.lock().unwrap().push(node);
        Box::pin(async { Ok(()) })
    }

    fn request<'a>(
        &'a self,
        node: NodeId,
        _choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<UpgradeRequest, String>> {
        self.asked.lock().unwrap().push(node);
        let failing = self.failing.lock().unwrap().contains(&node);
        let flashed = self.setup_node.collected.lock().unwrap().is_some();
        let mut request = request(node, Some(self.image.clone()), &[]);
        if flashed {
            request.from = "25.12.6".to_owned();
        }
        request.fetch = Arc::new(Fetch(Some(self.image.clone())));
        Box::pin(async move {
            if failing {
                Err("ASU's build failed".to_owned())
            } else {
                Ok(request)
            }
        })
    }
}

struct Lab {
    setup: Setup,
    rollouts: Rollouts,
    images: Arc<Images>,
    /// The adopted node, then the records added.
    nodes: Vec<NodeId>,
    _dir: tempfile::TempDir,
}

/// The adopted node with `role`, plus `more` access points pinned to the same device, all
/// managed; the driver started.
async fn lab(role: NodeRole, more: usize) -> Lab {
    let setup = setup_timed(true, timing()).await;
    let adopted = managed(&setup).await;
    setup.store.set_node_role(adopted, role).await.unwrap();
    let pin = setup.store.host_key(adopted).await.unwrap().unwrap();
    let mut nodes = vec![adopted];
    for i in 0..more {
        let node = setup
            .store
            .create_node(NewNode {
                name: format!("ap-{i}"),
                mac: MacAddr::new([2, 0, 0, 0, 9, u8::try_from(i).unwrap()]),
                board_name: "x86-64".to_owned(),
                host: format!("10.0.0.{}", 10 + i),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap();
        setup
            .store
            .pin_host_key(node.id, pin.clone())
            .await
            .unwrap();
        setup
            .store
            .set_node_mode(node.id, NodeMode::Managed)
            .await
            .unwrap();
        nodes.push(node.id);
    }
    play(&setup.node, GOOD);
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("image.img");
    std::fs::write(&image, IMAGE).unwrap();
    let images = Arc::new(Images {
        setup_node: Arc::clone(&setup.node),
        image,
        ahead: Mutex::default(),
        asked: Mutex::default(),
        failing: Mutex::default(),
    });
    let rollouts = Rollouts::new(
        setup.jobs.clone(),
        RolloutTiming {
            retry: Duration::from_millis(20),
            look: Duration::from_millis(20),
        },
    );
    rollouts.start(Arc::clone(&images) as Arc<dyn RolloutImages>);
    Lab {
        setup,
        rollouts,
        images,
        nodes,
        _dir: dir,
    }
}

fn asu() -> UpgradeChoice {
    UpgradeChoice::Asu {
        release: "25.12.6".to_owned(),
    }
}

/// The rollout once `until` holds.
async fn until(lab: &Lab, rollout: &Rollout, until: impl Fn(&Rollout) -> bool) -> Rollout {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let now = lab.rollouts.get(rollout.id).await.unwrap().unwrap();
            if until(&now) {
                return now;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the rollout gets there: {rollout:#?}"))
}

fn states(rollout: &Rollout) -> Vec<RolloutNodeState> {
    rollout.nodes.iter().map(|node| node.state).collect()
}

#[tokio::test]
async fn leaves_go_first_the_canary_soaks_and_the_gateway_goes_last() {
    let lab = lab(NodeRole::Gateway, 2).await;
    let [gateway, ap0, ap1] = lab.nodes[..] else {
        unreachable!()
    };
    let official = UpgradeChoice::Official {
        release: "25.12.6".to_owned(),
    };
    let rollout = lab
        .rollouts
        .create(
            RolloutRequest {
                nodes: vec![(gateway, asu()), (ap1, official), (ap0, asu())],
                canary: Some(CanaryRequest {
                    node: Some(ap1),
                    soak_min: 60,
                }),
                window: None,
                facts: BTreeMap::new(),
            },
            None,
        )
        .await
        .unwrap();
    let order: Vec<NodeId> = rollout.nodes.iter().map(|node| node.node).collect();
    assert_eq!(
        order,
        [ap1, ap0, gateway],
        "the canary, the leaves, the gateway"
    );
    assert_eq!(rollout.nodes[2].from, "24.10.8");

    // The canary upgrades, then soaks: nothing else starts.
    let soaking = until(&lab, &rollout, |r| {
        matches!(r.waiting, Some(RolloutWait::Canary { .. }))
    })
    .await;
    assert_eq!(
        states(&soaking),
        [
            RolloutNodeState::Upgraded,
            RolloutNodeState::Pending,
            RolloutNodeState::Pending
        ]
    );
    assert!(
        soaking.nodes[0]
            .detail
            .as_deref()
            .unwrap()
            .ends_with("soaking as the canary")
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*lab.images.asked.lock().unwrap(), [ap1]);
    let mut ahead = lab.images.ahead.lock().unwrap().clone();
    ahead.sort();
    assert_eq!(ahead, [gateway, ap0], "ASU is asked for its images at once");

    lab.setup.clock.advance(Duration::from_hours(1));
    let done = until(&lab, &rollout, |r| r.state == RolloutState::Done).await;
    assert_eq!(*lab.images.asked.lock().unwrap(), [ap1, ap0, gateway]);
    assert!(
        done.nodes
            .iter()
            .all(|node| node.state == RolloutNodeState::Upgraded)
    );
    assert!(
        done.nodes[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("Healthy after its soak, running 25.12.6"),
        "{:?}",
        done.nodes[0].detail
    );
    assert!(done.finished_at.is_some());
    for node in &done.nodes {
        let job = lab
            .setup
            .store
            .job(node.job.unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (job.kind, job.state),
            (JobKind::Upgrade, JobState::Succeeded)
        );
    }
    assert!(lab.setup.store.active_rollout().await.unwrap().is_none());
}

#[tokio::test]
async fn a_failure_halts_until_the_node_is_skipped_or_retried() {
    let lab = lab(NodeRole::Gateway, 1).await;
    let [gateway, ap] = lab.nodes[..] else {
        unreachable!()
    };
    lab.images.failing.lock().unwrap().insert(ap);
    let rollout = lab
        .rollouts
        .create(
            RolloutRequest {
                nodes: vec![(gateway, asu()), (ap, asu())],
                canary: None,
                window: None,
                facts: BTreeMap::new(),
            },
            None,
        )
        .await
        .unwrap();
    let halted = until(&lab, &rollout, |r| r.state == RolloutState::Halted).await;
    assert_eq!(
        states(&halted),
        [RolloutNodeState::Failed, RolloutNodeState::Pending]
    );
    assert_eq!(
        halted.error.as_deref(),
        Some("ap-0 couldn't start its upgrade: ASU's build failed")
    );

    // One rollout at a time; edits that don't fit the state are refused.
    let again = RolloutRequest {
        nodes: vec![(gateway, asu())],
        canary: None,
        window: None,
        facts: BTreeMap::new(),
    };
    assert!(matches!(
        lab.rollouts.create(again, None).await,
        Err(Error::Refused(_))
    ));
    assert!(matches!(
        lab.rollouts.pause(rollout.id).await,
        Err(Error::Refused(_))
    ));
    assert!(matches!(
        lab.rollouts.skip(rollout.id, NodeId::new(99)).await,
        Err(Error::Refused(_))
    ));

    // Retried, it fails again; skipped, the rest goes on.
    lab.rollouts.resume(rollout.id).await.unwrap();
    until(&lab, &rollout, |r| r.state == RolloutState::Halted).await;
    assert_eq!(lab.images.asked.lock().unwrap().len(), 2);
    lab.rollouts.skip(rollout.id, ap).await.unwrap();
    let resumed = lab.rollouts.resume(rollout.id).await.unwrap();
    assert!(resumed.error.is_none());
    let done = until(&lab, &rollout, |r| r.state == RolloutState::Done).await;
    assert_eq!(
        states(&done),
        [RolloutNodeState::Skipped, RolloutNodeState::Upgraded]
    );
    assert!(matches!(
        lab.rollouts.cancel(rollout.id).await,
        Err(Error::Refused(_))
    ));
}

#[tokio::test]
async fn nodes_start_only_inside_the_window_and_a_pause_holds_the_next() {
    let lab = lab(NodeRole::Gateway, 1).await;
    let [gateway, ap] = lab.nodes[..] else {
        unreachable!()
    };
    // The clock starts at midnight UTC, the site's time without a site.
    let rollout = lab
        .rollouts
        .create(
            RolloutRequest {
                nodes: vec![(gateway, asu()), (ap, asu())],
                canary: Some(CanaryRequest {
                    node: None,
                    soak_min: 30,
                }),
                window: Some(WINDOW),
                facts: BTreeMap::new(),
            },
            None,
        )
        .await
        .unwrap();
    let waiting = until(&lab, &rollout, |r| r.waiting.is_some()).await;
    let opens = lab
        .setup
        .clock
        .now()
        .saturating_add(Duration::from_mins(210));
    assert_eq!(waiting.waiting, Some(RolloutWait::Window { opens }));
    assert!(lab.images.asked.lock().unwrap().is_empty());

    // Paused, the window opening starts nothing; resumed, the first node goes.
    lab.rollouts.pause(rollout.id).await.unwrap();
    lab.setup.clock.advance(Duration::from_mins(210));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(lab.images.asked.lock().unwrap().is_empty());
    lab.rollouts.resume(rollout.id).await.unwrap();
    let soaking = until(&lab, &rollout, |r| {
        matches!(r.waiting, Some(RolloutWait::Canary { .. }))
    })
    .await;
    assert_eq!(soaking.nodes[0].node, ap);

    // The soak ends after the window closed: the gateway waits for the next night.
    lab.setup.clock.advance(Duration::from_hours(2));
    let next_night = until(&lab, &rollout, |r| {
        matches!(r.waiting, Some(RolloutWait::Window { .. }))
    })
    .await;
    assert_eq!(next_night.nodes[1].state, RolloutNodeState::Pending);
    assert!(lab.rollouts.cancel(rollout.id).await.is_ok());
    let cancelled = lab.rollouts.get(rollout.id).await.unwrap().unwrap();
    assert_eq!(cancelled.state, RolloutState::Cancelled);
    assert_eq!(cancelled.nodes[1].state, RolloutNodeState::Skipped);
}

#[tokio::test]
async fn an_upgrade_interrupted_by_a_restart_halts_the_rollout_on_its_node() {
    let setup = setup_timed(true, timing()).await;
    let node = managed(&setup).await;
    let job = setup
        .store
        .create_job(NewJob {
            kind: JobKind::Upgrade,
            target: "OpenWrt".to_owned(),
            node: Some(node),
            revision: None,
            created_by: None,
            steps: Vec::new(),
            nodes: Vec::new(),
        })
        .await
        .unwrap();
    let rollout = setup
        .store
        .create_rollout(NewRollout {
            nodes: vec![RolloutNode {
                node,
                name: "OpenWrt".to_owned(),
                choice: asu(),
                from: "24.10.8".to_owned(),
                state: RolloutNodeState::Upgrading,
                job: Some(job.id),
                detail: None,
            }],
            canary: None,
            window: None,
            created_by: None,
        })
        .await
        .unwrap();
    setup
        .store
        .interrupt_jobs(JobError {
            code: JobErrorCode::Interrupted,
            message: "The controller stopped while the job ran".to_owned(),
        })
        .await
        .unwrap();
    let rollouts = Rollouts::new(setup.jobs.clone(), RolloutTiming::default());
    let dir = tempfile::tempdir().unwrap();
    rollouts.start(Arc::new(Images {
        setup_node: Arc::clone(&setup.node),
        image: dir.path().join("none"),
        ahead: Mutex::default(),
        asked: Mutex::default(),
        failing: Mutex::default(),
    }));
    let halted = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let now = rollouts.get(rollout.id).await.unwrap().unwrap();
            if now.state == RolloutState::Halted {
                return now;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(halted.nodes[0].state, RolloutNodeState::Failed);
    assert_eq!(
        halted.error.as_deref(),
        Some("OpenWrt's upgrade failed: The controller stopped while the job ran")
    );
}

//! Replacing a node's device and re-adopting a reset one (D-131), over the scripted node of
//! [`crate::testing`].

use std::{net::Ipv4Addr, time::Duration};

use uniwrt_core::{Clock as _, MacAddr, NodeId};
use uniwrt_model::{
    job::{Job, JobErrorCode, JobKind, JobState, StepName, StepState},
    node::NodeMode,
    site::{
        SiteConfig,
        network::Reservation,
        node::{NodeConfig, PortConfig, TaggedNetworks},
        revision::{NewRevision, RevisionKind},
    },
    ztp::ZtpApproval,
};
use uniwrt_store::{JobRepo as _, NodeRepo as _, RevisionRepo as _, UserRepo as _, ZtpRepo as _};

use crate::{
    Author,
    testing::{COLLECTED, PASSWORD, Setup, setup, states},
};

const OLD_MAC: &str = "1e:09:3b:df:16:4c";
const NEW_MAC: &str = "1e:09:3b:df:16:99";

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

fn mac(text: &str) -> MacAddr {
    text.parse().unwrap()
}

/// A revision with port profiles for `node` (one on a port the device has) and a reservation of
/// its MAC.
async fn configured(setup: &Setup, node: NodeId) {
    let mut config = SiteConfig::defaults(None);
    let mut entry = NodeConfig::new(node);
    for port in ["eth0", "lan9"] {
        entry.ports.push(PortConfig {
            port: port.to_owned(),
            native: Some("lan".to_owned()),
            tagged: TaggedNetworks::None,
            backhaul: false,
        });
    }
    config.nodes.push(entry);
    config.networks[0]
        .ipv4
        .as_mut()
        .and_then(|v4| v4.dhcp.as_mut())
        .unwrap()
        .reservations
        .push(Reservation {
            mac: mac(OLD_MAC),
            ip: Ipv4Addr::new(10, 0, 0, 2),
            name: None,
        });
    setup
        .store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: None,
                author: None,
                message: "profiles".to_owned(),
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

#[tokio::test]
async fn a_new_device_takes_over_the_record_and_its_settings() {
    let (setup, id) = adopted().await;
    configured(&setup, id).await;
    let before = setup.store.node(id).await.unwrap().unwrap();
    *setup.node.collected.lock().unwrap() = Some(COLLECTED.replace(OLD_MAC, NEW_MAC));

    let by = admin(&setup).await;
    let started = setup
        .jobs
        .replace(id, setup.request(Some(PASSWORD)), Some(by.clone()))
        .await
        .unwrap();
    let job = finished(&setup, started).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!((job.kind, job.node), (JobKind::Replace, Some(id)));
    let record = job
        .steps
        .iter()
        .find(|step| step.name == StepName::Record)
        .unwrap();
    let detail = record.detail.as_deref().unwrap();
    assert!(detail.contains("Took over OpenWrt's record"), "{detail}");
    assert!(detail.contains("no port named lan9"), "{detail}");

    let nodes = setup.store.nodes().await.unwrap();
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    assert_eq!((node.id, node.name.as_str()), (id, before.name.as_str()));
    assert_eq!(node.mac, mac(NEW_MAC));
    assert_eq!(node.mode, NodeMode::Observe);

    let head = setup.store.head_revision().await.unwrap().unwrap();
    assert_eq!(head.meta.kind, RevisionKind::Replace);
    assert_eq!(head.meta.author.as_deref(), Some("admin"));
    assert_eq!(head.meta.message, "Replaced OpenWrt's device");
    let entry = head.config.nodes.iter().find(|n| n.node == id).unwrap();
    let ports: Vec<_> = entry.ports.iter().map(|p| p.port.as_str()).collect();
    assert_eq!(ports, ["eth0"]);
    let reservations = &head.config.networks[0]
        .ipv4
        .as_ref()
        .unwrap()
        .dhcp
        .as_ref()
        .unwrap()
        .reservations;
    assert_eq!(reservations[0].mac, mac(NEW_MAC));

    let watched = setup.watched.0.lock().unwrap();
    assert_eq!(watched.last().unwrap().0.mac, mac(NEW_MAC));
}

#[tokio::test]
async fn a_device_adopted_as_another_node_replaces_nothing() {
    let (setup, id) = adopted().await;
    let other = setup
        .store
        .create_node(uniwrt_model::node::NewNode {
            name: "attic".to_owned(),
            mac: mac("02:00:00:00:00:07"),
            board_name: "b".to_owned(),
            host: "10.0.0.7".to_owned(),
            port: 22,
            role: uniwrt_model::node::NodeRole::Ap,
        })
        .await
        .unwrap();
    let started = setup
        .jobs
        .replace(other.id, setup.request(Some(PASSWORD)), None)
        .await
        .unwrap();
    let job = finished(&setup, started).await;
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::AlreadyAdopted);
    assert_eq!(
        setup.store.node(other.id).await.unwrap().unwrap().mac,
        other.mac
    );
    assert_eq!(
        setup.store.node(id).await.unwrap().unwrap().mac,
        mac(OLD_MAC)
    );
}

#[tokio::test]
async fn a_reset_node_awaits_its_device_and_any_adoption_brings_it_back() {
    let (setup, id) = adopted().await;
    let started = setup
        .jobs
        .reset(id, Some(admin(&setup).await))
        .await
        .unwrap();
    let job = finished(&setup, started).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(
        states(&job),
        [
            (StepName::Connect, StepState::Done),
            // Observed: no transaction tool to remove.
            (StepName::RemoveTool, StepState::Skipped),
            (StepName::Wipe, StepState::Done),
            (StepName::Record, StepState::Done),
        ]
    );
    let approvals = setup.store.ztp_approvals(setup.clock.now()).await.unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].node, Some(id));
    assert_eq!(approvals[0].board_name.as_deref(), Some("container"));
    assert_eq!(approvals[0].expires_at, ZtpApproval::NEVER);
    // Still shown, offline, until the device is back.
    assert_eq!(setup.watched.1.lock().unwrap().last().unwrap().id, id);

    // Another board with its MAC isn't it.
    let mut other = approvals[0].clone();
    other.board_name = Some("glinet,gl-mt6000".to_owned());
    setup.store.put_ztp_approval(other).await.unwrap();
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.error.unwrap().code, JobErrorCode::IdentityMismatch);
    setup
        .store
        .put_ztp_approval(approvals[0].clone())
        .await
        .unwrap();

    // An ordinary adoption finds the record awaiting it.
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(job.node, Some(id));
    let record = job
        .steps
        .iter()
        .find(|step| step.name == StepName::Record)
        .unwrap();
    assert!(
        record
            .detail
            .as_deref()
            .unwrap()
            .starts_with("Adopted back into OpenWrt's record"),
        "{record:?}"
    );
    assert_eq!(setup.store.nodes().await.unwrap().len(), 1);
    assert_eq!(
        setup.store.ztp_approvals(setup.clock.now()).await.unwrap(),
        []
    );
    // Used once: the device is adopted again.
    let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
    assert_eq!(job.error.unwrap().code, JobErrorCode::AlreadyAdopted);
}

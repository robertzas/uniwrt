//! The adoption port end to end in demo mode (features/adoption.md, Path C; D-107, D-108): the
//! sim's bench device on the gateway's `lan4` is found through the gateway's login, adopted as
//! an access point (staged, lent to the LAN, confirmed at its lease) and configured with the
//! revision the gateway holds.

use std::{sync::Arc, time::Duration};

use uniwrt_core::{NodeId, SystemClock};
use uniwrt_jobs::{AdoptRequest, Jobs, PortLook, PortPath, ZtpClaim};
use uniwrt_model::{
    job::{Job, JobState, StepName, StepState},
    node::NodeRole,
    site::{
        adoption::AdoptionPort,
        revision::{NewRevision, RevisionKind},
    },
    status::NodeState,
};
use uniwrt_node::{ControllerKey, HostKey, Target};
use uniwrt_sim::SimConfig;
use uniwrt_store::{MasterKey, NodeRepo as _, RevisionRepo as _, Store, Vault};

use crate::fleet::Fleet;

async fn finished(jobs: &Jobs, job: Job, within: Duration) -> Job {
    tokio::time::timeout(within, async {
        loop {
            let job = jobs.job(job.id).await.unwrap().unwrap();
            if job.state != JobState::Running {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the job finishes")
}

/// The import holds the gateway's segment; the gateway, managed, holds the revision once applied
/// (nothing changes on it), which Configure applies to the adopted device.
async fn hold_the_site(fleet: &Fleet, store: &Store, jobs: &Jobs, vault: &Vault, gateway: NodeId) {
    let first = fleet.first_site().unwrap().unwrap();
    let mut site = first.config;
    assert_eq!(site.adoption_port, Some(AdoptionPort::new("lan4")));
    let mut sealed = Vec::new();
    for (placeholder, value) in first.secrets {
        let secret = vault.seal_config_secret(value.as_bytes()).unwrap();
        site.replace_secret(&placeholder, secret.reference());
        sealed.push(secret);
    }
    let managed = jobs.manage(gateway, true, None).await.unwrap();
    let managed = finished(jobs, managed, Duration::from_secs(20)).await;
    assert_eq!(managed.state, JobState::Succeeded, "{:?}", managed.error);
    let revision = store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Import,
                restored_from: None,
                author_id: None,
                author: None,
                message: "the demo".to_owned(),
                config: site,
            },
            sealed,
        )
        .await
        .unwrap();
    let apply = jobs.apply(revision.id, None).await.unwrap();
    let apply = finished(jobs, apply, Duration::from_secs(120)).await;
    assert_eq!(apply.state, JobState::Succeeded, "{:?}", apply.error);
}

#[tokio::test]
async fn the_bench_device_is_found_adopted_through_the_gateway_and_configured() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let config = SimConfig {
        reboot_every: None,
        bench: true,
        ..SimConfig::default()
    };
    let key = ControllerKey::generate().unwrap();
    let fleet = Fleet::demo(&store, &config, &key, clock.clone(), None)
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let jobs = fleet
        .jobs(&store, vault.clone(), &key, clock)
        .await
        .unwrap();
    let gateway = store.nodes().await.unwrap()[0].clone();
    assert_eq!(gateway.role, NodeRole::Gateway);

    hold_the_site(&fleet, &store, &jobs, &vault, gateway.id).await;

    // Plugged in a little after the start: found through the gateway's login.
    let found = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            match jobs.look_on_adoption_port(gateway.id).await.unwrap() {
                PortLook::Found(device) => return device,
                PortLook::Nothing => tokio::time::sleep(Duration::from_secs(1)).await,
                other => panic!("{other:?}"),
            }
        }
    })
    .await
    .expect("the bench device is found");
    assert!(!found.locked);
    assert_eq!(found.hostname, "OpenWrt");

    let request = AdoptRequest {
        target: Target::new(found.address.to_string(), 22),
        host_key: HostKey::parse(&found.host_key).unwrap(),
        password: None,
        remember_password: false,
        name: Some("ap-bench".to_owned()),
        role: Some(NodeRole::Ap),
        ztp: Some(ZtpClaim {
            mac: found.mac,
            board_name: found.board_name.clone(),
        }),
        port: Some(PortPath {
            gateway: gateway.id,
        }),
    };
    let adopted = jobs.adopt(request, None).await.unwrap();
    let adopted = finished(&jobs, adopted, Duration::from_secs(300)).await;
    assert_eq!(adopted.state, JobState::Succeeded, "{adopted:#?}");
    // The factory image lacks the mesh's packages, which Configure doesn't bring yet (M5.4
    // c2c2b): the device is adopted, with the staging config.
    for step in &adopted.steps {
        if step.name == StepName::Configure {
            let detail = step.detail.as_deref().unwrap_or_default();
            assert_eq!(step.state, StepState::Failed, "{step:#?}");
            assert!(detail.contains("the full wpad"), "{detail}");
        } else {
            assert_eq!(step.state, StepState::Done, "{step:#?}");
        }
    }
    let id = adopted.node.unwrap();
    let node = store.node(id).await.unwrap().unwrap();
    assert!(node.host.starts_with("10.0.0."), "{}", node.host);
    tokio::time::timeout(Duration::from_secs(10), async {
        while fleet.engine.node(id).map(|view| view.status.state) != Some(NodeState::Online) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the adopted node comes online");
    let link = fleet.sim_link(&node.host).unwrap();
    assert!(link.trusts(&key.authorized_key().unwrap()));

    jobs.shutdown().await;
    tokio::time::timeout(Duration::from_secs(5), fleet.stop())
        .await
        .unwrap();
    store.close().await;
}

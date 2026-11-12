//! The wired backhaul end to end in demo mode (features/mesh.md, Wired backhaul; D-097): the
//! demo's plain cable between the living room's and the office's access points, both managed,
//! comes over as an overlay in the first transaction, passes the probe from both ends, and
//! carries the mesh alone after the backhaul round.

use std::{sync::Arc, time::Duration};

use uniwrt_core::SystemClock;
use uniwrt_jobs::Jobs;
use uniwrt_model::{
    job::{Job, JobState, StepName, StepState},
    site::{
        node::ManagedPackage,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_node::{ControllerKey, NodeLink as _};
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

#[tokio::test]
async fn the_demo_cable_comes_over_to_the_wired_backhaul() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SystemClock);
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let config = SimConfig {
        reboot_every: None,
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
    let nodes = store.nodes().await.unwrap();
    let aps: Vec<_> = ["ap-living", "ap-office"]
        .map(|name| nodes.iter().find(|n| n.name == name).unwrap().clone())
        .into();

    // The import's secrets sealed, as the API commits the demo's first revision.
    let first = fleet.first_site().unwrap().unwrap();
    let mut site = first.config;
    let mut sealed = Vec::new();
    for (placeholder, value) in first.secrets {
        let secret = vault.seal_config_secret(value.as_bytes()).unwrap();
        site.replace_secret(&placeholder, secret.reference());
        sealed.push(secret);
    }
    for ap in &aps {
        let managed = jobs.manage(ap.id, true, None).await.unwrap();
        let managed = finished(&jobs, managed, Duration::from_secs(20)).await;
        assert_eq!(managed.state, JobState::Succeeded, "{:?}", managed.error);
        let node = site.nodes.iter_mut().find(|n| n.node == ap.id).unwrap();
        node.managed = [ManagedPackage::Network].into();
    }
    let revision = store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Import,
                restored_from: None,
                author_id: None,
                author: None,
                message: "the access points' network".to_owned(),
                config: site,
            },
            sealed,
        )
        .await
        .unwrap();
    let apply = jobs.apply(revision.id, None).await.unwrap();
    let apply = finished(&jobs, apply, Duration::from_secs(300)).await;
    assert_eq!(apply.state, JobState::Succeeded, "{:?}", apply.error);
    let backhaul = apply
        .steps
        .iter()
        .find(|step| step.name == StepName::Backhaul)
        .expect("a backhaul round");
    assert_eq!(backhaul.state, StepState::Done, "{backhaul:?}");
    let detail = backhaul.detail.as_deref().unwrap_or_default();
    assert!(detail.contains("carries the mesh alone"), "{detail}");

    // Each end holds the cable alone: its VLAN in batman-adv, the port out of the bridge.
    for (ap, port) in aps.iter().zip(["eth1", "eth0"]) {
        let link = fleet.sim_link(&ap.host).unwrap();
        let args =
            serde_json::value::to_raw_value(&serde_json::json!({"path": "/etc/config/network"}))
                .unwrap();
        let read = link
            .call("read_file", &args, Duration::from_secs(5))
            .await
            .unwrap();
        let read: serde_json::Value = serde_json::from_str(read.get()).unwrap();
        let network = uniwrt_uci::parse("network", read["data"].as_str().unwrap()).unwrap();
        let bridged = network
            .sections_of("device")
            .filter(|device| device.option("name") == Some("br-lan"))
            .filter_map(|device| device.get("ports"))
            .any(|ports| ports.items().iter().any(|p| p == port));
        let hardif = network.section(&format!("uw_bh_{port}"));
        let device = hardif.and_then(|iface| iface.option("device"));
        assert_eq!(device, Some(format!("{port}.4094").as_str()), "{}", ap.name);
        assert!(!bridged, "{}'s {port} left the bridge", ap.name);
    }

    jobs.shutdown().await;
    tokio::time::timeout(Duration::from_secs(5), fleet.stop())
        .await
        .unwrap();
    store.close().await;
}

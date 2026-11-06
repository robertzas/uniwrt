//! WAN tests over the scripted fleet, against a fake ISP that serves DHCP on VLAN 201 and,
//! when asked, delegates a prefix or answers 6rd.

use std::time::Duration;

use serde_json::{Value, json};
use uniwrt_model::{
    job::{Job, JobErrorCode, JobState, StepName, StepState},
    node::{NodeMode, NodeRole},
    site::gateway::{WanConfig, WanIpv6},
};
use uniwrt_sim::txn::Tool;
use uniwrt_store::NodeRepo as _;

use super::WanTest;
use crate::{
    Error,
    apply::tests::fleet::{Fleet, commit, fleet, output},
};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");

/// What the fake ISP offers.
#[derive(Clone, Copy)]
struct Isp {
    pd: bool,
    sixrd: bool,
}

/// The gateway's `interface` sections as (name, proto, device).
fn interfaces(tool: &Tool) -> Vec<(String, String, String)> {
    let Some(text) = tool.files.get("network") else {
        return Vec::new();
    };
    let Ok(network) = uniwrt_uci::parse("network", text) else {
        return Vec::new();
    };
    network
        .sections_of("interface")
        .map(|s| {
            let get = |o| s.option(o).unwrap_or_default().to_owned();
            (
                s.name.clone().unwrap_or_default(),
                get("proto"),
                get("device"),
            )
        })
        .collect()
}

fn iface(name: &str, proto: &str, extra: &Value) -> Value {
    let mut iface = json!({
        "interface": name, "up": true, "pending": false, "available": true, "autostart": true,
        "dynamic": false, "uptime": 10, "l3_device": "eth1.201", "proto": proto,
        "device": "eth1.201", "ipv4-address": [], "ipv6-address": [], "ipv6-prefix": [],
        "ipv6-prefix-assignment": [], "route": [], "dns-server": [], "dns-search": [],
        "neighbors": [], "data": {},
    });
    iface
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    iface
}

impl Isp {
    fn serve(self, fleet: &Fleet) {
        let fake = fleet.fake(0);
        *fake.collect_hook.lock().unwrap() = Some(Box::new(move |tool, collected| {
            let sets = &mut collected["sets"];
            sets["devices"]["sys"]["eth1"] = json!({
                "type": "1", "address": "02:00:00:00:00:99", "mtu": "1500", "flags": "0x1003",
                "carrier": "1", "speed": "1000", "duplex": "full", "operstate": "up",
                "uevent": "INTERFACE=eth1", "master": null, "wireless": false, "physical": true,
            });
            let list = sets["interfaces"]["dump"]["interface"]
                .as_array_mut()
                .unwrap();
            for (name, proto, device) in interfaces(tool) {
                let on_vlan = device == "eth1.201";
                match (name.as_str(), proto.as_str()) {
                    ("wan", "dhcp") if on_vlan => list.push(iface("wan", "dhcp", &json!({
                        "ipv4-address": [{"address": "203.0.113.10", "mask": 24}],
                        "route": [{"target": "0.0.0.0", "mask": 0, "nexthop": "203.0.113.1", "source": "203.0.113.10/32"}],
                        "dns-server": ["203.0.113.53"],
                    }))),
                    ("wan6", "dhcpv6") if on_vlan && self.pd => list.push(iface("wan6", "dhcpv6", &json!({
                        "ipv6-prefix": [{"address": "2001:db8:4a:100::", "mask": 56}],
                    }))),
                    ("wan6", "6rd") if self.sixrd => list.push(iface("wan6", "6rd", &json!({
                        "ipv6-address": [{"address": "2602:cb00:710a::1", "mask": 64}],
                    }))),
                    _ => {}
                }
            }
        }));
        *fake.exec_hook.lock().unwrap() = Some(Box::new(|_, argv| match argv[0].as_str() {
            "ping" => Some(output(0, "round-trip min/avg/max = 1.0/2.5/4.0 ms\n")),
            "wget" => Some(output(0, "")),
            _ => None,
        }));
    }
}

fn lumen() -> WanConfig {
    WanConfig {
        vlan: Some(201),
        preset: Some("us-lumen-ipoe".to_owned()),
        ..WanConfig::default()
    }
}

fn sixrd() -> WanIpv6 {
    WanIpv6::SixRd {
        prefix: "2602::/24".parse().unwrap(),
        border_relay: "205.171.2.64".parse().unwrap(),
        ipv4_mask_len: 0,
    }
}

async fn test(fleet: &Fleet, wan: WanConfig, fallback: Option<WanIpv6>) -> Job {
    let mut updates = fleet.jobs.subscribe();
    let test = WanTest {
        wan,
        password: None,
        fallback,
    };
    let started = fleet.jobs.test_wan(test, None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let job = updates.recv().await.unwrap();
            if job.id == started.id && job.state != JobState::Running {
                return (*job).clone();
            }
        }
    })
    .await
    .expect("the test ends")
}

fn step(job: &Job, name: StepName) -> (StepState, String) {
    let step = job.steps.iter().find(|s| s.name == name).unwrap();
    (step.state, step.detail.clone().unwrap_or_default())
}

#[tokio::test]
async fn a_working_wan_passes_every_step_and_is_rolled_back() {
    let fleet = fleet(&[GW]).await;
    Isp {
        pd: true,
        sixrd: false,
    }
    .serve(&fleet);
    let before = fleet.fake(0).file("network");
    let job = test(&fleet, lumen(), Some(sixrd())).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    for name in [
        StepName::Link,
        StepName::Address,
        StepName::Gateway,
        StepName::Dns,
        StepName::Https,
        StepName::Ipv6,
        StepName::Rollback,
    ] {
        assert_eq!(step(&job, name).0, StepState::Done, "{name:?}: {job:#?}");
    }
    assert_eq!(step(&job, StepName::Address).1, "Lease 203.0.113.10/24");
    assert!(
        step(&job, StepName::Dns)
            .1
            .contains("203.0.113.53 (the ISP's)")
    );
    assert!(
        step(&job, StepName::Ipv6)
            .1
            .starts_with("Prefix 2001:db8:4a:100::/56")
    );
    assert_eq!(step(&job, StepName::SixRd).0, StepState::Skipped);
    // Rolled back: the gateway holds its own network again.
    assert_eq!(fleet.fake(0).file("network"), before);
    let journal = fleet.journal();
    let commands: Vec<&str> = journal.iter().filter_map(|l| l.split(' ').nth(1)).collect();
    assert_eq!(commands, ["stage", "put", "check", "apply", "rollback"]);
}

#[tokio::test]
async fn a_wrong_vlan_fails_with_the_presets_hint() {
    let fleet = fleet(&[GW]).await;
    Isp {
        pd: true,
        sixrd: false,
    }
    .serve(&fleet);
    let wan = WanConfig {
        vlan: None,
        ..lumen()
    };
    let job = test(&fleet, wan, None).await;
    let error = job.error.clone().unwrap();
    assert_eq!(error.code, JobErrorCode::NoInternet);
    assert!(
        error.message.contains("transparent bridge mode"),
        "{error:?}"
    );
    assert_eq!(step(&job, StepName::Address).0, StepState::Failed);
    assert_eq!(step(&job, StepName::Https).0, StepState::Skipped);
    assert_eq!(step(&job, StepName::Rollback).0, StepState::Done);
}

#[tokio::test]
async fn without_a_prefix_the_6rd_fallback_is_tried() {
    let fleet = fleet(&[GW]).await;
    Isp {
        pd: false,
        sixrd: true,
    }
    .serve(&fleet);
    let before = fleet.fake(0).file("network");
    let job = test(&fleet, lumen(), Some(sixrd())).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    assert_eq!(step(&job, StepName::Ipv6).0, StepState::Failed);
    let (state, detail) = step(&job, StepName::SixRd);
    assert_eq!(state, StepState::Done, "{detail}");
    assert!(detail.starts_with("6rd works"), "{detail}");
    assert_eq!(fleet.fake(0).file("network"), before);
}

#[tokio::test]
async fn an_observed_gateway_is_not_tested() {
    let fleet = fleet(&[GW]).await;
    let id = fleet.nodes[0].0.id;
    fleet
        .store
        .set_node_mode(id, NodeMode::Observe)
        .await
        .unwrap();
    let test = WanTest {
        wan: lumen(),
        password: None,
        fallback: None,
    };
    assert!(matches!(
        fleet.jobs.test_wan(test, None).await,
        Err(Error::Observed(node)) if node == id
    ));
}

#[tokio::test]
async fn a_gateway_that_manages_no_package_is_tested_all_the_same() {
    let fleet = fleet(&[GW]).await;
    Isp {
        pd: true,
        sixrd: false,
    }
    .serve(&fleet);
    let head = fleet.head().await;
    let mut site = head.config.clone();
    site.nodes.clear();
    commit(&fleet.store, Some(head.meta.id), site).await;
    let job = test(&fleet, lumen(), None).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
}

//! The port path of the adopt job (Path C, D-107) over a fake gateway and the factory device
//! behind it: staged through the tunnel, the port lent, confirmed at the lease, recorded
//! managed; the port given back and the device rolled back when it fails.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use uniwrt_core::NodeId;
use uniwrt_model::{
    job::{Job, JobErrorCode, JobState, StepState},
    node::{NodeMode, NodeRole},
};
use uniwrt_node::{ExecOutput, ExitStatus, Target, txn::TOOL};
use uniwrt_store::NodeRepo as _;

use super::{AdoptRequest, PortPath};
use crate::{
    ApplyTiming,
    adoption_port_tests::{UP, gateway, ok},
    testing::{COLLECTED, FakeNode, Setup, setup_timed, states},
};

const NETWORK: &str = "\
config interface 'loopback'
\toption device 'lo'
\toption proto 'static'
\toption ipaddr '127.0.0.1'
\toption netmask '255.0.0.0'

config device
\toption name 'br-lan'
\toption type 'bridge'
\tlist ports 'eth0'

config interface 'lan'
\toption device 'br-lan'
\toption proto 'static'
\toption ipaddr '192.168.1.1'
\toption netmask '255.255.255.0'
\toption ip6assign '60'

config interface 'wan'
\toption device 'eth1'
\toption proto 'dhcp'
";
const DHCP: &str = "\
config dnsmasq
\toption domainneeded '1'

config dhcp 'lan'
\toption interface 'lan'
\toption start '100'
";
/// The device's bridge MAC, as its collected facts show it (the capture's, renumbered).
const DEVICE_MAC: &str = "2e:09:3b:df:16:4c";
const LEASE: &str = "10.0.0.57";

fn timing() -> ApplyTiming {
    ApplyTiming {
        poll: Duration::from_millis(20),
        verify: Duration::from_millis(600),
        timer_uplink: Duration::from_secs(2),
        ..ApplyTiming::default()
    }
}

/// What the gateway was asked to do with its port, in order.
type Lends = Arc<Mutex<Vec<String>>>;

/// A gateway with the adoption port `eth2` that lends it and has `leases`.
async fn site(setup: &Setup, leases: &str) -> (NodeId, Lends) {
    let gw = gateway(setup, Some("eth2"), UP).await;
    let lends = Lends::default();
    let seen = Arc::clone(&lends);
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv| match argv {
        ["ubus", "call", "network.interface.uw_adopt", "status"] => Some(Ok(ok(UP))),
        ["sh", "-c", _, "sh", verb, rest @ ..] => {
            seen.lock()
                .unwrap()
                .push(format!("{verb} {}", rest.join(" ")));
            Some(Ok(ok(match *verb {
                "lend" => "lent\n",
                "keep" => "kept\n",
                _ => "ended\n",
            })))
        }
        _ => None,
    }));
    let collected = format!(
        r#"{{"sets":{{"leases":{{"now":1000,"dnsmasq":[{{"data":"{leases}"}}]}}}},"errors":{{}}}}"#
    );
    *setup.node.collected.lock().unwrap() = Some(collected);
    (gw, lends)
}

/// A factory device behind the gateway; once it applies, it leaves 192.168.1.1 and, when
/// `leaves`, answers at [`LEASE`].
fn device(setup: &Setup, leaves: bool) -> Arc<FakeNode> {
    let device = FakeNode::factory();
    *device.collected.lock().unwrap() = Some(COLLECTED.replace("1e:09:3b", "2e:09:3b"));
    let (gw, me) = (Arc::clone(&setup.node), Arc::downgrade(&device));
    *device.script.lock().unwrap() = Some(Box::new(move |argv| {
        let me = me.upgrade()?;
        let answer = match argv {
            ["sh", "-c", _, "sh", "network", "dhcp", "wireless"] => format!(
                "network {}\n{NETWORK}dhcp {}\n{DHCP}wireless -\n",
                NETWORK.len(),
                DHCP.len()
            ),
            [tool, verb, id, ..] if *tool == TOOL => match *verb {
                "stage" | "confirm" | "rollback" => {
                    if *verb != "stage" {
                        *me.open_txn.lock().unwrap() = None;
                    }
                    r#"{"ok":{}}"#.to_owned()
                }
                "put" => r#"{"ok":{"bytes":1}}"#.to_owned(),
                "check" => r#"{"ok":{"packages":["dhcp","network"]}}"#.to_owned(),
                "apply" => {
                    *me.open_txn.lock().unwrap() = Some((*id).to_owned());
                    if leaves {
                        *gw.behind.lock().unwrap() = None;
                        gw.hosts.lock().unwrap().insert(LEASE.to_owned(), me);
                    }
                    r#"{"ok":{"timeout_s":2}}"#.to_owned()
                }
                _ => return None,
            },
            _ => return None,
        };
        Some(Ok(ExecOutput {
            status: ExitStatus::Code(0),
            stdout: answer.into_bytes(),
            stderr: Vec::new(),
        }))
    }));
    *setup.node.behind.lock().unwrap() = Some(Arc::clone(&device));
    device
}

fn request(device: &FakeNode, gateway: NodeId, role: Option<NodeRole>) -> AdoptRequest {
    AdoptRequest {
        target: Target::new("192.168.1.1", 22),
        host_key: device.host_key.clone(),
        password: None,
        remember_password: false,
        name: Some("Attic".to_owned()),
        role,
        ztp: None,
        port: Some(PortPath { gateway }),
    }
}

/// The verbs the device's transaction tool ran, in order.
fn tool_verbs(device: &FakeNode) -> Vec<String> {
    let commands = device.commands.lock().unwrap();
    commands
        .iter()
        .filter(|argv| argv[0] == TOOL)
        .map(|argv| argv[1].clone())
        .collect()
}

fn failed(job: &Job) -> (JobErrorCode, &str) {
    let error = job.error.as_ref().expect("an error");
    (error.code, error.message.as_str())
}

#[tokio::test]
async fn a_device_on_the_port_is_staged_lent_confirmed_at_its_lease_and_managed() {
    let setup = setup_timed(true, timing()).await;
    let (gw, lends) = site(&setup, &format!("0 {DEVICE_MAC} {LEASE} OpenWrt *\\n")).await;
    let device = device(&setup, true);
    let (job, _) = setup.adopt(request(&device, gw, Some(NodeRole::Ap))).await;
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.error);
    assert!(
        states(&job)
            .iter()
            .all(|(_, state)| *state == StepState::Done),
        "{:?}",
        states(&job)
    );
    assert_eq!(
        tool_verbs(&device),
        ["stage", "put", "put", "check", "apply", "confirm"]
    );
    assert!(!device.password.lock().unwrap().is_empty());
    let lends = lends.lock().unwrap().clone();
    assert_eq!(lends.len(), 2, "{lends:?}");
    assert!(lends[0].starts_with("lend eth2 lan "), "{lends:?}");
    assert_eq!(lends[1], "keep eth2");

    let node = setup.store.node(job.node.unwrap()).await.unwrap().unwrap();
    assert_eq!(
        (node.name.as_str(), node.host.as_str(), node.role, node.mode),
        ("Attic", LEASE, NodeRole::Ap, NodeMode::Managed)
    );
    let watched = setup.watched.0.lock().unwrap();
    assert_eq!(watched.last().map(|(node, ..)| node.id), Some(node.id));
}

#[tokio::test]
async fn a_device_without_a_lease_gives_the_port_back_and_rolls_back_by_itself() {
    let setup = setup_timed(true, timing()).await;
    let (gw, lends) = site(&setup, "").await;
    let device = device(&setup, true);
    let (job, _) = setup
        .adopt(request(&device, gw, Some(NodeRole::Bridge)))
        .await;
    assert_eq!(job.state, JobState::Failed);
    let (code, message) = failed(&job);
    assert_eq!(code, JobErrorCode::NodeFailed);
    assert!(message.contains("took no lease"), "{message}");
    assert!(
        message.contains("back on the adoption segment"),
        "{message}"
    );
    let lends = lends.lock().unwrap().clone();
    assert_eq!(
        lends.last().map(String::as_str),
        Some("end eth2"),
        "{lends:?}"
    );
    assert_eq!(
        tool_verbs(&device).last().map(String::as_str),
        Some("apply")
    );
    assert!(
        setup
            .store
            .nodes()
            .await
            .unwrap()
            .iter()
            .all(|n| n.id == gw)
    );
}

#[tokio::test]
async fn a_device_that_stays_at_the_factory_address_is_rolled_back_before_any_lend() {
    let setup = setup_timed(true, timing()).await;
    let (gw, lends) = site(&setup, "").await;
    let device = device(&setup, false);
    let (job, _) = setup.adopt(request(&device, gw, Some(NodeRole::Ap))).await;
    assert_eq!(failed(&job).0, JobErrorCode::RolledBack);
    assert_eq!(
        tool_verbs(&device).last().map(String::as_str),
        Some("rollback")
    );
    assert!(lends.lock().unwrap().is_empty());
}

#[tokio::test]
async fn only_access_points_and_bridges_come_through_the_port() {
    let setup = setup_timed(true, timing()).await;
    let (gw, _) = site(&setup, "").await;
    let device = device(&setup, true);
    for role in [None, Some(NodeRole::Gateway)] {
        let (job, _) = setup.adopt(request(&device, gw, role)).await;
        assert_eq!(failed(&job).0, JobErrorCode::Refused);
    }
    assert_eq!(device.commands(), Vec::<String>::new());
}

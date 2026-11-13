//! Path C end to end (features/adoption.md; D-107): the adopt job takes `fac`, stock OpenWrt on
//! the gateway's adoption port, through the gateway's login (the dial's tunnel reaches it
//! only while the test says it is behind the gateway), stages it with the real transaction
//! tool, has the gateway lend the port to the LAN, finds its lease in the gateway's leases,
//! confirms there and records it managed at that address; an apply job of its own then gives it
//! the revision the gateway holds, at the same address. `fac` is put back as it shipped.

use std::collections::BTreeMap;

use uniwrt_jobs::{AdoptRequest, PortPath};
use uniwrt_model::{
    job::{JobState, StepName, StepState},
    node::{NodeMode, NodeRole},
    site::adoption::AdoptionPort,
};
use uniwrt_node::{Target, adoption_port, txn};
use uniwrt_store::{ApplyRepo as _, NodeRepo as _};
use uniwrt_system::{Node, ipv4, site::GATEWAY};

use super::{FAC, GW, UP, lab_with};

/// Gives the port back and puts `fac` back as it shipped, whatever the test's outcome.
struct Restore {
    gw: Node,
    fac: Node,
    factory: BTreeMap<String, uniwrt_uci::Package>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        self.gw.exec(&adoption_port::end_lend("eth2"));
        self.fac.exec(&txn::uninstall().request);
        let on = BTreeMap::from([("dnsmasq", true), ("odhcpd", true), ("firewall", true)]);
        self.fac.write_packages(&self.factory, &on);
        // `reload_config` alone left it at its lease after a confirmed staging (seen on 25.12).
        self.fac.run("ubus call network reload", &[], &[]);
        self.fac
            .wait_interface("lan", UP, |s| ipv4(s) == ["192.168.1.1/24"]);
    }
}

fn segment_up(gw: &Node) {
    gw.wait_interface("uw_adopt", UP, |s| ipv4(s) == ["192.168.1.250/24"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs scripts/test-system.sh's topology"]
async fn a_factory_device_on_the_adoption_port_is_adopted_through_the_gateway() {
    let lab = lab_with(&[FAC]).await;
    let revision = lab
        .revise(|config| config.adoption_port = Some(AdoptionPort::new("eth2")))
        .await;
    let applied = lab.apply(revision).await;
    assert_eq!(applied.state, JobState::Succeeded, "{applied:#?}");
    let fac = Node::new(FAC);
    let restore = tokio::task::block_in_place(|| {
        segment_up(&lab.gw);
        Restore {
            gw: Node::new(GW),
            fac: Node::new(FAC),
            factory: fac.packages(),
        }
    });

    lab.dial.behind(FAC);
    let request = AdoptRequest {
        target: Target::new("192.168.1.1", 22),
        host_key: lab.dial.host_key(FAC),
        password: None,
        remember_password: false,
        name: Some("fac".to_owned()),
        role: Some(NodeRole::Ap),
        ztp: None,
        port: Some(PortPath { gateway: GATEWAY }),
    };
    let started = lab
        .jobs
        .adopt(request, None)
        .await
        .expect("the adoption starts");
    let job = lab.wait(started).await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    let node = lab
        .store
        .node(job.node.expect("recorded"))
        .await
        .expect("the store")
        .expect("the node");
    assert!(node.host.starts_with("10.0.0."), "{}", node.host);
    assert_eq!((node.role, node.mode), (NodeRole::Ap, NodeMode::Managed));
    let configure = job.steps.last().expect("steps");
    assert_eq!(
        (configure.name, configure.state),
        (StepName::Configure, StepState::Done),
        "{job:#?}"
    );
    let held = lab.store.applied_revisions().await.expect("the store");
    let holds = |id| held.iter().find(|a| a.node == id).map(|a| a.revision);
    assert_eq!(holds(node.id), Some(revision));
    assert_eq!(holds(GATEWAY), Some(revision));

    tokio::task::block_in_place(|| {
        // At its lease, with its own configuration confirmed and the port still lent.
        let at = format!("{}/", node.host);
        fac.wait_interface("lan", UP, |s| ipv4(s).iter().any(|a| a.starts_with(&at)));
        let status = fac.sh(&format!("{} status", txn::TOOL), &[]);
        assert!(status.contains(r#""open":null"#), "{status}");
        let lent = "[ -e /sys/class/net/br-lan/brif/eth2 ]";
        assert!(lab.gw.run(lent, &[], &[]).ok, "the lend isn't kept");
        drop(restore);
        segment_up(&lab.gw);
    });
}

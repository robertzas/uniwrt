//! Looks on the adoption port over a fake gateway and the fake device behind it.

use std::sync::Arc;

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    node::NodeRole,
    site::{
        SiteConfig,
        adoption::AdoptionPort,
        revision::{NewRevision, RevisionKind},
    },
    ztp::PendingSource,
};
use uniwrt_node::{ExecOutput, ExitStatus};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, ZtpRepo as _};

use crate::{
    Error, PortLook,
    testing::{FakeNode, PASSWORD, Setup, setup},
};

pub(crate) const UP: &str = r#"{"up":true,"ipv4-address":[{"address":"192.168.1.250","mask":24}]}"#;
const ARP: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         02:42:ac:11:00:03     *        br-adopt
";
const BOARD: &str = r#"{"model":"GL.iNet GL-MT6000","hostname":"OpenWrt",
    "board_name":"glinet,gl-mt6000","release":{"version":"24.10.4"}}"#;
const LABEL: &str = "94:83:c4:00:00:09";

pub(crate) fn ok(stdout: &str) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

/// Adopts the fake node as the gateway, saves a site with `port` as its adoption port, and has
/// the gateway answer for its segment with `status`.
pub(crate) async fn gateway(setup: &Setup, port: Option<&str>, status: &'static str) -> NodeId {
    let mut request = setup.request(Some(PASSWORD));
    request.role = Some(NodeRole::Gateway);
    let (job, _) = setup.adopt(request).await;
    let mut config = SiteConfig::defaults(None);
    config.adoption_port = port.map(AdoptionPort::new);
    setup
        .store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: None,
                author: None,
                message: "adoption port".to_owned(),
                config,
            },
            Vec::new(),
        )
        .await
        .unwrap();
    *setup.node.script.lock().unwrap() = Some(Box::new(move |argv| match argv {
        ["ubus", "call", "network.interface.uw_adopt", "status"] => Some(Ok(ok(status))),
        ["cat", "/proc/net/arp"] => Some(Ok(ok(ARP))),
        _ => None,
    }));
    job.node.unwrap()
}

/// A factory-default device behind the gateway that shows its board and `identity`.
fn device(setup: &Setup, identity: &'static str) -> Arc<FakeNode> {
    let device = FakeNode::factory();
    *device.script.lock().unwrap() = Some(Box::new(move |argv| match argv {
        ["ubus", "call", "system", "board"] => Some(Ok(ok(BOARD))),
        ["sh", "-c", _] => Some(Ok(ok(identity))),
        _ => None,
    }));
    *setup.node.behind.lock().unwrap() = Some(Arc::clone(&device));
    device
}

#[tokio::test]
async fn a_factory_device_on_the_port_lands_in_the_inbox_unchanged() {
    let setup = setup(true).await;
    let gw = gateway(&setup, Some("eth2"), UP).await;
    let device = device(&setup, "00:00:00:00:00:00\n94:83:C4:00:00:09\n");
    let PortLook::Found(found) = setup.jobs.look_on_adoption_port(gw).await.unwrap() else {
        panic!("nothing found");
    };
    assert_eq!(found.mac, LABEL.parse::<MacAddr>().unwrap());
    assert_eq!(
        (found.board_name.as_str(), found.model.as_str()),
        ("glinet,gl-mt6000", "GL.iNet GL-MT6000")
    );
    assert_eq!(
        (found.hostname.as_str(), found.release.as_str()),
        ("OpenWrt", "24.10.4")
    );
    assert_eq!(found.address.to_string(), "192.168.1.1");
    assert_eq!(
        found.source,
        PendingSource::Port {
            gateway: gw,
            port: "eth2".to_owned()
        }
    );
    assert!(!found.locked);
    assert_eq!(found.host_key, device.host_key.to_openssh());
    assert_eq!(found.role, None);
    assert_eq!(
        setup.store.ztp_pending().await.unwrap(),
        std::slice::from_ref(&*found)
    );
    // Only reads ran on it.
    assert_eq!(device.commands(), ["ubus", "sh"]);

    // Looking again finds it again; the row stays one.
    let look = setup.jobs.look_on_adoption_port(gw).await.unwrap();
    assert!(matches!(look, PortLook::Found(ref again) if again.first_seen == found.first_seen));
    assert_eq!(setup.store.ztp_pending().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_device_with_a_root_password_is_listed_locked_by_its_arp_entry() {
    let setup = setup(true).await;
    let gw = gateway(&setup, Some("eth2"), UP).await;
    let device = device(&setup, LABEL);
    *device.password.lock().unwrap() = "set".to_owned();
    let look = setup.jobs.look_on_adoption_port(gw).await.unwrap();
    let PortLook::Found(found) = look else {
        panic!("{look:?}");
    };
    assert!(found.locked);
    assert_eq!(found.mac, "02:42:ac:11:00:03".parse::<MacAddr>().unwrap());
    assert_eq!((found.model.as_str(), found.release.as_str()), ("", ""));
    assert_eq!(found.host_key, device.host_key.to_openssh());
    assert!(device.commands().is_empty(), "no login, so nothing ran");
}

#[tokio::test]
async fn no_look_without_the_port_its_segment_or_the_gateway() {
    let setup = setup(true).await;
    let gw = gateway(&setup, Some("eth2"), UP).await;
    let look = |node| {
        let jobs = setup.jobs.clone();
        async move { jobs.look_on_adoption_port(node).await.unwrap() }
    };
    // Nothing answers yet.
    assert_eq!(look(gw).await, PortLook::Nothing);
    // Another node.
    device(&setup, LABEL);
    assert_eq!(look(NodeId::new(99)).await, PortLook::NotThePort);
    // An adopted node answers.
    let mac = setup.store.node(gw).await.unwrap().unwrap().mac;
    let adopted: &'static str = Box::leak(mac.to_string().into_boxed_str());
    device(&setup, adopted);
    assert_eq!(look(gw).await, PortLook::Adopted(gw));
    // The gateway out of reach.
    let keys = std::mem::take(&mut *setup.node.authorized_keys.lock().unwrap());
    assert_eq!(look(gw).await, PortLook::Unreachable);
    *setup.node.authorized_keys.lock().unwrap() = keys;
    assert_eq!(setup.store.ztp_pending().await.unwrap(), []);

    // The segment down, or not in the saved site.
    let down = crate::testing::setup(true).await;
    let gw = gateway(&down, Some("eth2"), r#"{"up":false}"#).await;
    device(&down, LABEL);
    let look = down.jobs.look_on_adoption_port(gw).await.unwrap();
    assert_eq!(look, PortLook::NotThePort);
    let unset = crate::testing::setup(true).await;
    let gw = gateway(&unset, None, UP).await;
    device(&unset, LABEL);
    let look = unset.jobs.look_on_adoption_port(gw).await.unwrap();
    assert_eq!(look, PortLook::NotThePort);

    let offline = crate::testing::setup(false).await;
    assert!(matches!(
        offline.jobs.look_on_adoption_port(gw).await,
        Err(Error::Unavailable)
    ));
}

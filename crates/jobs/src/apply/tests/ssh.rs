//! SSH hardening through the apply (features/security.md, Node hardening; D-150): it applies
//! like any change, and a node whose dropbear doesn't run as the file asks rolls back.

use uniwrt_model::{
    job::{JobErrorCode, JobState},
    node::NodeRole,
    site::node::{ManagedPackage, SshHardening},
};

use super::fleet::{commit, fleet};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

/// The fleet's head revision with every node key-only and bound.
async fn hardened(fleet: &super::fleet::Fleet) {
    let head = fleet.head().await;
    let mut site = head.config;
    for node in &mut site.nodes {
        node.managed.insert(ManagedPackage::Dropbear);
        node.ssh = SshHardening {
            key_only: true,
            management_only: true,
        };
    }
    commit(&fleet.store, Some(head.meta.id), site).await;
}

#[tokio::test]
async fn hardening_applies_and_the_servers_follow_it() {
    let fleet = fleet(&[GW, AP]).await;
    hardened(&fleet).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
    for i in 0..2 {
        let file = fleet.fake(i).file("dropbear").unwrap();
        assert!(file.contains("option PasswordAuth 'off'"), "{file}");
        assert!(file.contains("option DirectInterface 'lan'"), "{file}");
    }
}

#[tokio::test]
async fn a_server_still_taking_passwords_rolls_the_apply_back() {
    let fleet = fleet(&[GW, AP]).await;
    let before = fleet.fake(1).file("dropbear");
    // The AP's dropbear keeps running as it did: the file changed, the server didn't.
    *fleet.fake(1).collect_hook.lock().unwrap() = Some(Box::new(|_, collected| {
        collected["sets"]["services"]["ssh"] = serde_json::json!([
            {"running": true, "command": ["/usr/sbin/dropbear", "-F", "-p", "22"]}
        ]);
    }));
    hardened(&fleet).await;
    let job = fleet.apply().await;
    assert_eq!(job.state, JobState::Failed, "{job:#?}");
    let error = job.error.unwrap();
    assert_eq!(error.code, JobErrorCode::RolledBack);
    assert!(
        error.message.contains("SSH doesn't run as applied"),
        "{}",
        error.message
    );
    assert_eq!(fleet.fake(1).file("dropbear"), before);
}

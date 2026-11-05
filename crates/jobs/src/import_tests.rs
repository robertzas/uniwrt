//! Imports over the scripted fleet of the apply tests: every adopted node read, the secrets
//! found sealed once, nodes not reached left unread.

use std::sync::atomic::Ordering;

use uniwrt_model::{
    import::{NodeImportState, SectionFate},
    node::{NodeMode, NodeRole},
    site::{
        node::RawValue,
        revision::{NewRevision, RevisionKind},
    },
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _};

use crate::{
    Error, ImportDraft,
    apply::tests::fleet::{Fleet, fleet},
};

const GW: (NodeRole, &str) = (NodeRole::Gateway, "192.168.1.1");
const AP: (NodeRole, &str) = (NodeRole::Ap, "192.168.1.21");

const WIREGUARD: &str = "
config interface 'wg0'
\toption proto 'wireguard'
\toption private_key 'cHJpdmF0ZQ=='
\tlist addresses '10.9.0.1/24'
";

/// The fleet, its gateway holding OpenWrt 25.12's stock packages and a WireGuard tunnel.
async fn brownfield() -> Fleet {
    let fleet = fleet(&[GW, AP]).await;
    let mut tool = fleet.fake(0).tool.lock().unwrap();
    for (name, text) in [
        (
            "system",
            include_str!("../../uci/fixtures/openwrt/25.12/system"),
        ),
        (
            "network",
            include_str!("../../uci/fixtures/openwrt/25.12/network"),
        ),
        (
            "dhcp",
            include_str!("../../uci/fixtures/openwrt/25.12/dhcp"),
        ),
        (
            "firewall",
            include_str!("../../uci/fixtures/openwrt/25.12/firewall"),
        ),
    ] {
        tool.files.insert(name.to_owned(), text.to_owned());
    }
    tool.files.get_mut("network").unwrap().push_str(WIREGUARD);
    drop(tool);
    fleet
}

async fn accept(fleet: &Fleet, draft: ImportDraft) {
    fleet
        .store
        .commit_revision(
            NewRevision {
                base: Some(draft.preview.base),
                kind: RevisionKind::Import,
                restored_from: None,
                author_id: None,
                author: None,
                message: "imported".to_owned(),
                config: draft.site,
            },
            draft.sealed,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn every_node_is_read_its_secrets_sealed_once() {
    let fleet = brownfield().await;
    let ap = fleet.nodes[1].0.id;
    fleet
        .store
        .set_node_mode(ap, NodeMode::Observe)
        .await
        .unwrap();
    let draft = fleet.jobs.import().await.unwrap();
    let preview = &draft.preview;
    let head = fleet.store.head_revision().await.unwrap().unwrap();
    assert_eq!(preview.base, head.meta.id);
    assert_eq!(
        preview
            .nodes
            .iter()
            .map(|n| (n.role, n.state))
            .collect::<Vec<_>>(),
        [
            (NodeRole::Gateway, NodeImportState::Imported),
            (NodeRole::Ap, NodeImportState::Imported)
        ],
        "{:#?}",
        preview.nodes
    );
    let gateway = &preview.nodes[0];
    assert!(gateway.confidence > 50, "{}", gateway.confidence);
    assert!(
        gateway
            .sections
            .iter()
            .any(|s| s.fate == SectionFate::Raw && s.name.as_deref() == Some("wg0"))
    );
    assert!(preview.changes(), "the stock LAN replaces the test site's");
    let raw = &draft
        .site
        .nodes
        .iter()
        .find(|n| n.node == gateway.node)
        .unwrap()
        .raw;
    let wg0 = raw
        .iter()
        .find(|r| r.name.as_deref() == Some("wg0"))
        .unwrap();
    assert!(matches!(
        wg0.options["private_key"],
        RawValue::Secret { .. }
    ));
    let refs: Vec<_> = draft.sealed.iter().map(|s| s.reference().clone()).collect();
    assert!(
        draft
            .site
            .secrets()
            .iter()
            .filter_map(|slot| slot.value)
            .all(|secret| refs.contains(secret)
                || head
                    .config
                    .secrets()
                    .iter()
                    .any(|s| s.value == Some(secret))),
        "every secret is sealed or held already"
    );
    assert!(!draft.sealed.is_empty());
    accept(&fleet, draft).await;

    let again = fleet.jobs.import().await.unwrap();
    assert!(
        again.sealed.is_empty(),
        "the same values keep their references"
    );
    assert_eq!(again.preview.changes, Vec::new());
}

#[tokio::test]
async fn nodes_not_reached_are_left_unread() {
    let fleet = brownfield().await;
    fleet.fake(1).down.store(true, Ordering::SeqCst);
    let draft = fleet.jobs.import().await.unwrap();
    let ap = &draft.preview.nodes[1];
    assert_eq!(ap.state, NodeImportState::Unread);
    assert!(
        ap.reason.as_deref().unwrap().contains("Cannot log in"),
        "{ap:?}"
    );
    assert_eq!(draft.preview.nodes[0].state, NodeImportState::Imported);
}

#[tokio::test]
async fn controllers_without_a_dial_cannot_import() {
    let fleet = brownfield().await;
    drop(fleet.jobs.import().await.unwrap());
    let setup = crate::testing::setup(false).await;
    assert!(matches!(setup.jobs.import().await, Err(Error::Unavailable)));
}

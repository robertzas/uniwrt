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
    Error, ImportDraft, ImportScope,
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
    let draft = fleet.jobs.import(None).await.unwrap();
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

    let again = fleet.jobs.import(None).await.unwrap();
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
    let draft = fleet.jobs.import(None).await.unwrap();
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
    drop(fleet.jobs.import(None).await.unwrap());
    let setup = crate::testing::setup(false).await;
    assert!(matches!(
        setup.jobs.import(None).await,
        Err(Error::Unavailable)
    ));
}

#[tokio::test]
async fn a_scoped_import_takes_only_the_drifted_package() {
    let fleet = brownfield().await;
    let draft = fleet.jobs.import(None).await.unwrap();
    accept(&fleet, draft).await;
    let gateway = fleet.nodes[0].0.id;
    {
        let mut tool = fleet.fake(0).tool.lock().unwrap();
        let dhcp = tool.files.get_mut("dhcp").unwrap();
        *dhcp = dhcp.replace("option domain 'lan'", "option domain 'home'");
        let system = tool.files.get_mut("system").unwrap();
        *system = system.replace("option zonename 'UTC'", "option zonename 'Europe/Berlin'");
    }
    let scope = ImportScope {
        node: gateway,
        package: "dhcp".to_owned(),
    };
    let draft = fleet.jobs.import(Some(&scope)).await.unwrap();
    let paths: Vec<String> = draft
        .preview
        .changes
        .iter()
        .map(|change| change.path.join("."))
        .collect();
    assert!(paths.iter().any(|p| p.ends_with("domain")), "{paths:?}");
    assert!(!paths.iter().any(|p| p.contains("time")), "{paths:?}");

    // A package that didn't drift imports as nothing.
    let unchanged = ImportScope {
        node: fleet.nodes[1].0.id,
        package: "wireless".to_owned(),
    };
    let draft = fleet.jobs.import(Some(&unchanged)).await.unwrap();
    assert_eq!(draft.preview.changes, Vec::new());

    fleet.fake(0).down.store(true, Ordering::SeqCst);
    assert!(matches!(
        fleet.jobs.import(Some(&scope)).await,
        Err(Error::Failed(why)) if why.contains("Cannot read")
    ));
}

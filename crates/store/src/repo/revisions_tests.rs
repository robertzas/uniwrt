//! Revisions over a real database: the linear history, conflicts, diffs, paging, and secrets
//! committed with their revision and only referable from its descendants or a restore.

use std::time::Duration;

use uniwrt_core::RevisionId;
use uniwrt_model::site::{
    SecretRef, SiteConfig,
    diff::ChangeOp,
    revision::{NewRevision, RevisionKind},
    wireless::{NodeScope, Wlan, WlanSecurity},
};

use crate::{
    Error, MasterKey, RevisionRepo as _, Vault, VaultRepo as _,
    testing::{START, open},
};

fn edit(base: Option<RevisionId>, config: SiteConfig, message: &str) -> NewRevision {
    NewRevision {
        base,
        kind: if base.is_some() {
            RevisionKind::Edit
        } else {
            RevisionKind::Defaults
        },
        restored_from: None,
        author_id: None,
        author: Some("admin".to_owned()),
        message: message.to_owned(),
        config,
    }
}

fn wlan(passphrase: &SecretRef) -> Wlan {
    Wlan {
        id: "home".to_owned(),
        ssid: "Home".to_owned(),
        enabled: true,
        network: "lan".to_owned(),
        security: WlanSecurity::Wpa2Wpa3,
        passphrase: Some(passphrase.clone()),
        bands: vec![],
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
    }
}

#[tokio::test]
async fn a_linear_history_with_diffs_and_conflicts() {
    let (_dir, store, clock) = open().await;
    assert!(store.head_revision().await.unwrap().is_none());
    let defaults = SiteConfig::defaults(None);
    let first = store
        .commit_revision(edit(None, defaults.clone(), "Defaults"), vec![])
        .await
        .unwrap();
    assert_eq!(
        (first.parent, first.changes, first.created_at),
        (None, 0, START)
    );
    // A second first revision conflicts.
    let err = store
        .commit_revision(edit(None, defaults.clone(), "again"), vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotLatest { .. }), "{err}");

    clock.advance(Duration::from_secs(60));
    let mut renamed = defaults.clone();
    renamed.site.name = "Cabin".to_owned();
    renamed.networks[0].vlan = Some(10);
    let second = store
        .commit_revision(edit(Some(first.id), renamed.clone(), "Rename"), vec![])
        .await
        .unwrap();
    assert_eq!((second.parent, second.changes), (Some(first.id), 2));
    // Saving from the first again is stale.
    let err = store
        .commit_revision(edit(Some(first.id), defaults.clone(), "stale"), vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotLatest { .. }), "{err}");

    let head = store.head_revision().await.unwrap().unwrap();
    assert_eq!((head.meta.clone(), head.config), (second.clone(), renamed));
    let paths: Vec<_> = head.diff.iter().map(|c| (c.path.join("/"), c.op)).collect();
    assert_eq!(
        paths,
        [
            ("networks/lan/vlan".to_owned(), ChangeOp::Changed),
            ("site/name".to_owned(), ChangeOp::Changed)
        ]
    );
    assert_eq!(
        store.revision(first.id).await.unwrap().unwrap().config,
        defaults
    );
    assert!(store.revision(RevisionId::new(99)).await.unwrap().is_none());

    assert_eq!(
        store.revisions(None, 10).await.unwrap(),
        [second.clone(), first.clone()]
    );
    assert_eq!(
        store.revisions(None, 1).await.unwrap(),
        std::slice::from_ref(&second)
    );
    assert_eq!(store.revisions(Some(second.id), 10).await.unwrap(), [first]);
}

#[tokio::test]
async fn secrets_are_committed_with_their_revision() {
    let (_dir, store, _clock) = open().await;
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    let first = store
        .commit_revision(edit(None, SiteConfig::defaults(None), "Defaults"), vec![])
        .await
        .unwrap();

    // A reference to a secret nobody committed is refused.
    let mut config = SiteConfig::defaults(None);
    let made_up = SecretRef::new(format!("config/{}", "0".repeat(32))).unwrap();
    config.wlans.push(wlan(&made_up));
    let err = store
        .commit_revision(edit(Some(first.id), config.clone(), "made up"), vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::UnknownSecret { .. }), "{err}");

    let sealed = vault.seal_config_secret(b"correct horse").unwrap();
    let old = sealed.reference().clone();
    config.wlans[0] = wlan(&old);
    let second = store
        .commit_revision(edit(Some(first.id), config.clone(), "Wi-Fi"), vec![sealed])
        .await
        .unwrap();
    assert_eq!(
        vault.secret(old.name()).await.unwrap().unwrap().as_slice(),
        b"correct horse"
    );

    // The next revision may keep it; a new value gets a new name.
    let sealed = vault.seal_config_secret(b"battery staple").unwrap();
    let new = sealed.reference().clone();
    assert_ne!(new, old);
    config.wlans[0] = wlan(&new);
    let third = store
        .commit_revision(
            edit(Some(second.id), config.clone(), "New key"),
            vec![sealed],
        )
        .await
        .unwrap();
    assert_eq!(
        vault.secret(old.name()).await.unwrap().unwrap().as_slice(),
        b"correct horse"
    );

    // Restoring the second revision may bring its old key back.
    config.wlans[0] = wlan(&old);
    let err = store
        .commit_revision(
            edit(Some(third.id), config.clone(), "not a restore"),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::UnknownSecret { .. }), "{err}");
    let restore = NewRevision {
        kind: RevisionKind::Restore,
        restored_from: Some(second.id),
        ..edit(Some(third.id), config.clone(), "Restore")
    };
    let fourth = store.commit_revision(restore, vec![]).await.unwrap();
    assert_eq!(fourth.restored_from, Some(second.id));
    assert_eq!(fourth.kind, RevisionKind::Restore);

    // A failed commit leaves no secret behind.
    let sealed = vault.seal_config_secret(b"orphan").unwrap();
    let orphan = sealed.reference().clone();
    store
        .commit_revision(edit(Some(third.id), config, "stale"), vec![sealed])
        .await
        .unwrap_err();
    assert!(vault.secret(orphan.name()).await.unwrap().is_none());
}

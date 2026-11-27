//! What the compatibility fixtures hold ([`seed`]) and what every later uniwrt must read back
//! from them ([`check`]): the data a user would lose to a failed upgrade (the admin, the nodes
//! and their pinned host keys, the vault, the site's revision and its secrets, settings, a
//! notifier, a floor, a ZTP pre-approval), not the telemetry that ages out anyway. The constants
//! are part of the fixtures already written: never change them.

use std::{collections::BTreeSet, sync::Arc};

use serde_json::json;
use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::EventLevel,
    floor::FloorFields,
    node::{HostKeyPin, NewNode, NodeRole},
    notify::{NotifierConfig, NotifierFilter, NotifierSpec, NtfySpec},
    site::{
        SiteConfig,
        migrate::upgrade,
        revision::{NewRevision, RevisionKind},
    },
    user::Role,
    ztp::ZtpApproval,
};
use zeroize::Zeroizing;

use crate::{
    FloorRepo as _, HostKeyRepo as _, MasterKey, NodeRepo as _, NotifierRepo as _,
    RevisionRepo as _, SettingsRepo as _, Store, UserRepo as _, Vault, VaultRepo as _,
    ZtpRepo as _,
};

/// The clock of the seed and of the checks: 2026-11-27T00:00:00Z.
pub(super) const START: Timestamp = Timestamp::from_unix_secs(1_795_737_600);
pub(super) const MASTER_KEY: &str =
    "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";
const ADMIN: &str = "admin";
const PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$Y29tcGF0aWJsZQ$7b1u2v3w4x5y6z7A8B9C0D1E2F3G4H5I6J7K8L9M0N";
const GATEWAY: MacAddr = MacAddr::new([0x94, 0x83, 0xc4, 0xc0, 0x4a, 0x01]);
const AP: MacAddr = MacAddr::new([0x94, 0x83, 0xc4, 0xc0, 0x4a, 0x02]);
const PRE_APPROVED: MacAddr = MacAddr::new([0x94, 0x83, 0xc4, 0xc0, 0x4a, 0x03]);
const HOST_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGNvbXBhdGliaWxpdHkgZml4dHVyZSBob3N0IGtleQ";
const FINGERPRINT: &str = "SHA256:Y29tcGF0aWJpbGl0eSBmaXh0dXJlIGZpbmdlcnByaW50";
const SECRET: &str = "controller/compat";
const SECRET_VALUE: &[u8] = b"kept across every 1.x upgrade";
const SETTING: &str = "compat";
const MESSAGE: &str = "The compatibility fixture's site";
const NOTIFIER: &str = "Phone";
const TOPIC: &str = "uniwrt-compat";
const FLOOR: &str = "Ground floor";
const ZTP_NAME: &str = "Attic";

/// A clock at [`START`].
pub(super) fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(START))
}

/// The value the fixture's revision seals for its secret `name`.
fn config_secret(name: &str) -> Vec<u8> {
    format!("value of {name}").into_bytes()
}

/// Writes the fixture's data into `store`, a fresh store at the latest schema.
pub(super) async fn seed(store: &Store) {
    let admin = store
        .create_first_user(ADMIN, Role::Admin, PASSWORD_HASH)
        .await
        .unwrap();
    let node = |name: &str, mac, role| NewNode {
        name: name.to_owned(),
        mac,
        board_name: "glinet,gl-mt6000".to_owned(),
        host: "192.168.1.1".to_owned(),
        port: 22,
        role,
    };
    let gateway = store
        .create_node(node("gateway", GATEWAY, NodeRole::Gateway))
        .await
        .unwrap();
    store
        .create_node(node("hall", AP, NodeRole::Ap))
        .await
        .unwrap();
    store
        .pin_host_key(
            gateway.id,
            HostKeyPin {
                public_key: HOST_KEY.to_owned(),
                fingerprint: FINGERPRINT.to_owned(),
                pinned_at: START,
            },
        )
        .await
        .unwrap();
    let vault = Vault::open(store, &MasterKey::parse(MASTER_KEY).unwrap())
        .await
        .unwrap();
    vault
        .put_secret(SECRET, Zeroizing::new(SECRET_VALUE.to_vec()))
        .await
        .unwrap();
    let config = site();
    let names: BTreeSet<_> = config
        .secrets()
        .into_iter()
        .filter_map(|slot| slot.value.cloned())
        .collect();
    let secrets = names
        .into_iter()
        .map(|name| {
            let value = config_secret(name.name());
            vault.seal_config_secret_as(name, &value).unwrap()
        })
        .collect();
    store
        .commit_revision(
            NewRevision {
                base: None,
                kind: RevisionKind::Edit,
                restored_from: None,
                author_id: Some(admin.id),
                author: Some(ADMIN.to_owned()),
                message: MESSAGE.to_owned(),
                config,
            },
            secrets,
        )
        .await
        .unwrap();
    seed_settings(store, gateway.id).await;
}

/// The settings of [`seed`]: a setting, a notifier, a floor and a ZTP pre-approval.
async fn seed_settings(store: &Store, gateway: NodeId) {
    store
        .put_setting(SETTING, json!({ "kept": true }))
        .await
        .unwrap();
    store
        .create_notifier(NotifierConfig {
            name: NOTIFIER.to_owned(),
            enabled: true,
            spec: NotifierSpec::Ntfy(NtfySpec {
                server: "https://ntfy.sh".to_owned(),
                topic: TOPIC.to_owned(),
                username: None,
            }),
            filter: NotifierFilter {
                min: EventLevel::Warning,
                nodes: vec![gateway],
                resolved: true,
                remind_s: Some(3_600),
            },
        })
        .await
        .unwrap();
    store
        .create_floor(
            FloorFields {
                name: FLOOR.to_owned(),
                level: 0,
                height_m: 2.6,
                scale: None,
            },
            10,
        )
        .await
        .unwrap();
    store
        .put_ztp_approval(ZtpApproval {
            mac: PRE_APPROVED,
            name: Some(ZTP_NAME.to_owned()),
            role: Some(NodeRole::Ap),
            created_at: START,
            // 2100-01-01: never expired when checked.
            expires_at: Timestamp::from_unix_secs(4_102_444_800),
            created_by: Some(ADMIN.to_owned()),
            node: None,
            board_name: None,
        })
        .await
        .unwrap();
}

/// Reads back through `store` what [`seed`] wrote, whichever schema it wrote it at.
pub(super) async fn check(store: &Store) {
    let (admin, hash) = store.credentials(ADMIN).await.unwrap().expect("the admin");
    assert_eq!((admin.role, hash.as_str()), (Role::Admin, PASSWORD_HASH));

    let nodes = store.nodes().await.unwrap();
    let gateway = nodes
        .iter()
        .find(|node| node.mac == GATEWAY)
        .expect("the gateway");
    assert_eq!(
        (gateway.name.as_str(), gateway.role),
        ("gateway", NodeRole::Gateway)
    );
    assert!(
        nodes
            .iter()
            .any(|node| node.mac == AP && node.role == NodeRole::Ap)
    );
    let pin = store
        .host_key(gateway.id)
        .await
        .unwrap()
        .expect("its host key");
    assert_eq!(
        (pin.public_key.as_str(), pin.fingerprint.as_str()),
        (HOST_KEY, FINGERPRINT)
    );

    let vault = Vault::open(store, &MasterKey::parse(MASTER_KEY).unwrap())
        .await
        .expect("the vault opens with its key");
    let secret = vault.secret(SECRET).await.unwrap().expect("the secret");
    assert_eq!(secret.as_slice(), SECRET_VALUE);

    let head = store.head_revision().await.unwrap().expect("the revision");
    assert_eq!(head.meta.message, MESSAGE);
    let refs: Vec<_> = head
        .config
        .secrets()
        .into_iter()
        .filter_map(|slot| slot.value.cloned())
        .collect();
    assert!(!refs.is_empty(), "the site's secrets");
    for name in refs {
        let value = vault.secret(name.name()).await.unwrap();
        assert_eq!(
            value.as_deref().map(Vec::as_slice),
            Some(config_secret(name.name()).as_slice()),
            "{}",
            name.name()
        );
    }

    assert_eq!(
        store.setting(SETTING).await.unwrap(),
        Some(json!({ "kept": true }))
    );
    let notifiers = store.notifiers().await.unwrap();
    let [notifier] = notifiers.as_slice() else {
        panic!("one notifier, not {}", notifiers.len());
    };
    assert_eq!(notifier.config.name, NOTIFIER);
    assert!(matches!(&notifier.config.spec, NotifierSpec::Ntfy(spec) if spec.topic == TOPIC));
    assert_eq!(notifier.config.filter.nodes, vec![gateway.id]);
    let floors = store.floors().await.unwrap();
    assert_eq!(
        floors
            .iter()
            .map(|floor| floor.name.as_str())
            .collect::<Vec<_>>(),
        [FLOOR]
    );
    let approvals = store.ztp_approvals(START).await.unwrap();
    let [approval] = approvals.as_slice() else {
        panic!("one pre-approval, not {}", approvals.len());
    };
    assert_eq!(
        (approval.mac, approval.name.as_deref()),
        (PRE_APPROVED, Some(ZTP_NAME))
    );
}

/// The site the revision holds: the model's fixture of schema 21, upgraded.
fn site() -> SiteConfig {
    upgrade(serde_json::from_str(include_str!("../../../model/fixtures/site/v21.json")).unwrap())
        .unwrap()
}

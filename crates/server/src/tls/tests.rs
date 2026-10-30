//! [`start`] and the [`Renewer`] over a real vault, on a manual clock.

use std::{sync::Arc, time::Duration};

use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_store::{MasterKey, Store, Vault, VaultRepo as _};

use super::{CA_SECRET, LEAF_SECRET, Renewer, Tls, issue::RENEW_BEFORE, names::TlsName, start};

/// 2026-10-30T00:00:00Z.
const NOW: Timestamp = Timestamp::from_unix_secs(1_793_318_400);

struct Setup {
    _dir: tempfile::TempDir,
    store: Store,
    vault: Vault,
    clock: ManualClock,
}

async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(NOW);
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(clock.clone()))
        .await
        .unwrap();
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    Setup {
        _dir: dir,
        store,
        vault,
        clock,
    }
}

async fn start_with(setup: &Setup, configured: &[&str]) -> (Tls, Renewer) {
    let configured: Vec<TlsName> = configured.iter().map(|n| n.parse().unwrap()).collect();
    start(
        setup.vault.clone(),
        Arc::new(setup.clock.clone()),
        configured,
        vec!["127.0.0.1:8443".parse().unwrap()],
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn the_first_start_makes_both_and_later_starts_reuse_them() {
    let setup = setup().await;
    let (tls, _) = start_with(&setup, &[]).await;
    let first = tls.info();
    assert!(first.ca_name.starts_with("uniwrt local CA "));
    assert_eq!(first.ca_sha256.len(), 32 * 3 - 1);
    assert_eq!(first.ca_not_after, "2036-10-29T00:00:00Z");
    assert_eq!(first.not_after, "2027-12-01T00:00:00Z");
    assert!(first.names.iter().any(|n| n == "uniwrt.lan"));
    assert!(tls.ca_pem().starts_with("-----BEGIN CERTIFICATE-----"));
    assert!(setup.vault.secret(CA_SECRET).await.unwrap().is_some());
    let leaf = setup.vault.secret(LEAF_SECRET).await.unwrap();

    setup.clock.advance(Duration::from_hours(1));
    let (again, _) = start_with(&setup, &[]).await;
    assert_eq!(again.info().ca_sha256, first.ca_sha256);
    assert_eq!(again.info().not_after, first.not_after);
    assert_eq!(setup.vault.secret(LEAF_SECRET).await.unwrap(), leaf);
    setup.store.close().await;
}

#[tokio::test]
async fn new_names_reissue_the_leaf_under_the_same_ca_unless_it_may_not_sign_them() {
    let setup = setup().await;
    let (tls, _) = start_with(&setup, &[]).await;
    let ca = tls.info().ca_sha256;

    let (tls, _) = start_with(&setup, &["192.168.7.7", "nas.home.arpa"]).await;
    let info = tls.info();
    assert_eq!(info.ca_sha256, ca);
    assert!(info.names.iter().any(|n| n == "192.168.7.7"));
    assert!(info.names.iter().any(|n| n == "nas.home.arpa"));

    // Made before it was configured, the CA may not sign a public name: the leaf leaves it out.
    let (tls, _) = start_with(&setup, &["uniwrt.example.com"]).await;
    assert!(!tls.info().names.iter().any(|n| n == "uniwrt.example.com"));
    setup.store.close().await;
}

#[tokio::test]
async fn names_configured_on_the_first_run_are_permitted_to_the_ca() {
    let setup = setup().await;
    let (tls, _) = start_with(&setup, &["uniwrt.example.com", "203.0.113.9"]).await;
    let names = tls.info().names;
    assert!(names.iter().any(|n| n == "uniwrt.example.com"));
    assert!(names.iter().any(|n| n == "203.0.113.9"));
    setup.store.close().await;
}

#[tokio::test]
async fn the_renewer_swaps_in_a_new_leaf_once_one_is_due() {
    let setup = setup().await;
    let (tls, renewals) = start_with(&setup, &[]).await;
    let first = tls.info();
    assert!(!renewals.check().await.unwrap());

    let lifetime = Duration::from_hours(397 * 24);
    setup
        .clock
        .advance(lifetime.saturating_sub(RENEW_BEFORE) + Duration::from_secs(1));
    assert!(renewals.check().await.unwrap());
    let renewed = tls.info();
    assert_eq!(renewed.ca_sha256, first.ca_sha256);
    assert_eq!(renewed.not_after, "2028-12-02T00:00:01Z");
    assert!(!renewals.check().await.unwrap());

    // The renewed leaf is the one the vault holds for the next start.
    let (again, _) = start_with(&setup, &[]).await;
    assert_eq!(again.info().not_after, renewed.not_after);
    setup.store.close().await;
}

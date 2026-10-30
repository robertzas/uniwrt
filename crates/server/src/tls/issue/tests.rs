//! Certificates checked as a client checks them: webpki, with the CA as the only root.

use std::{sync::Arc, time::Duration};

use rustls::{
    RootCertStore,
    client::{WebPkiServerVerifier, danger::ServerCertVerifier as _},
    crypto::ring,
    pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject as _},
};
use uniwrt_core::Timestamp;

use super::{Ca, LEAF_LIFETIME, RENEW_BEFORE};
use crate::tls::names::{Host, Names, Permitted, TlsName};

/// 2026-10-30T00:00:00Z.
const NOW: Timestamp = Timestamp::from_unix_secs(1_793_318_400);

fn names(configured: &[&str]) -> Names {
    let configured: Vec<TlsName> = configured.iter().map(|n| n.parse().unwrap()).collect();
    let host = Host {
        hostname: Some("homeserver".into()),
        addresses: vec!["192.168.1.10".parse().unwrap()],
    };
    Names::wanted(&configured, &[], &host)
}

/// Verifies the leaf for `name` at `at`, as a client that trusts only the CA.
fn verify(ca: &Ca, leaf: &super::Leaf, name: &str, at: Timestamp) -> Result<(), rustls::Error> {
    let provider = Arc::new(ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(ca.der().unwrap()).unwrap();
    let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
        .unwrap();
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(leaf.cert.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap();
    let at = UnixTime::since_unix_epoch(Duration::from_millis(at.as_unix_ms().try_into().unwrap()));
    verifier
        .verify_server_cert(
            &chain[0],
            &[],
            &ServerName::try_from(name).unwrap(),
            &[],
            at,
        )
        .map(|_| ())
}

#[test]
fn the_leaf_verifies_for_each_of_its_names() {
    let wanted = names(&["uniwrt.example.com", "203.0.113.9"]);
    let ca = Ca::create(Permitted::for_ca(&wanted), NOW).unwrap();
    assert!(ca.name.starts_with("uniwrt local CA "), "{}", ca.name);
    let leaf = ca.issue(&wanted, NOW).unwrap();
    for name in [
        "localhost",
        "uniwrt.lan",
        "homeserver",
        "homeserver.lan",
        "uniwrt.example.com",
        "127.0.0.1",
        "::1",
        "192.168.1.10",
        "203.0.113.9",
    ] {
        verify(&ca, &leaf, name, NOW).unwrap_or_else(|err| panic!("{name}: {err}"));
    }
    for name in ["other.lan", "192.168.1.11"] {
        assert!(verify(&ca, &leaf, name, NOW).is_err(), "{name}");
    }
}

#[test]
fn the_constraints_stop_the_ca_from_signing_other_names() {
    let ca = Ca::create(Permitted::local(), NOW).unwrap();
    let mut names = Names::default();
    names.dns.insert("www.example.com".into());
    let leaf = ca.issue(&names, NOW).unwrap();
    let err = verify(&ca, &leaf, "www.example.com", NOW).unwrap_err();
    assert!(err.to_string().contains("NameConstraint"), "{err}");

    let mut names = Names::default();
    names.ips.insert("8.8.8.8".parse().unwrap());
    let leaf = ca.issue(&names, NOW).unwrap();
    assert!(verify(&ca, &leaf, "8.8.8.8", NOW).is_err());
}

#[test]
fn validity_starts_a_day_early_and_lasts_397_days() {
    let wanted = names(&[]);
    let ca = Ca::create(Permitted::for_ca(&wanted), NOW).unwrap();
    let leaf = ca.issue(&wanted, NOW).unwrap();
    assert_eq!(leaf.not_after, NOW.saturating_add(LEAF_LIFETIME));
    let hour = Duration::from_hours(1);
    verify(&ca, &leaf, "uniwrt.lan", NOW.saturating_sub(23 * hour)).unwrap();
    assert!(verify(&ca, &leaf, "uniwrt.lan", NOW.saturating_sub(25 * hour)).is_err());
    verify(
        &ca,
        &leaf,
        "uniwrt.lan",
        leaf.not_after.saturating_sub(hour),
    )
    .unwrap();
    assert!(
        verify(
            &ca,
            &leaf,
            "uniwrt.lan",
            leaf.not_after.saturating_add(hour)
        )
        .is_err()
    );
}

#[test]
fn a_leaf_is_renewed_when_it_nears_expiry_its_names_change_or_its_ca_does() {
    let wanted = names(&[]);
    let ca = Ca::create(Permitted::for_ca(&wanted), NOW).unwrap();
    let leaf = ca.issue(&wanted, NOW).unwrap();
    assert_eq!(leaf.renewal(&ca.name, &wanted, NOW), None);

    let late = leaf.not_after.saturating_sub(RENEW_BEFORE);
    assert_eq!(leaf.renewal(&ca.name, &wanted, late), None);
    let later = late.saturating_add(Duration::from_secs(1));
    assert_eq!(
        leaf.renewal(&ca.name, &wanted, later),
        Some("it expires soon")
    );

    assert_eq!(
        leaf.renewal(&ca.name, &names(&["10.9.9.9"]), NOW),
        Some("the names changed")
    );
    let other = Ca::create(Permitted::for_ca(&wanted), NOW).unwrap();
    assert_eq!(
        leaf.renewal(&other.name, &wanted, NOW),
        Some("its CA changed")
    );
}

#[test]
fn the_stored_forms_round_trip_and_serve() {
    let wanted = names(&[]);
    let ca = Ca::create(Permitted::for_ca(&wanted), NOW).unwrap();
    let leaf = ca.issue(&wanted, NOW).unwrap();
    let ca: Ca = serde_json::from_slice(&serde_json::to_vec(&ca).unwrap()).unwrap();
    let leaf: super::Leaf = serde_json::from_slice(&serde_json::to_vec(&leaf).unwrap()).unwrap();
    assert_eq!(leaf.renewal(&ca.name, &wanted, NOW), None);
    let key = leaf
        .certified_key(ca.der().unwrap(), &ring::default_provider())
        .unwrap();
    assert_eq!(key.cert.len(), 2);
    assert_eq!(key.cert[1], ca.der().unwrap());
}

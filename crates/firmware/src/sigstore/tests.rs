//! The verifier against a real attestation (v0.108.0's `x86_64` tarball, from GitHub's
//! attestations API), and that each altered part of it is refused.

use data_encoding::{BASE64, HEXLOWER};
use serde_json::Value;

use super::{
    Identity, Subject, TrustedRoot,
    rekor::{included, leaf_hash, node_hash},
    verify,
};

const ANSWER: &[u8] = include_bytes!("testdata/attestations-0.108.0.json");
const NAME: &str = "uniwrt-0.108.0-x86_64-unknown-linux-musl.tar.gz";
const SHA256: &str = "9bd81920268c3967a21cbe7c2ccd2deef4ef486847b5c6ec8286e38c74151267";

fn bundle() -> Value {
    let answer: Value = serde_json::from_slice(ANSWER).unwrap();
    answer["attestations"][0]["bundle"].clone()
}

fn sha256() -> [u8; 32] {
    HEXLOWER
        .decode(SHA256.as_bytes())
        .unwrap()
        .try_into()
        .unwrap()
}

fn check(bundle: &Value, name: &str, identity: Identity<'_>) -> Result<super::Provenance, String> {
    let digest = sha256();
    verify(
        &serde_json::to_vec(bundle).unwrap(),
        Subject {
            name,
            sha256: &digest,
        },
        identity,
        &TrustedRoot::embedded().unwrap(),
    )
    .map_err(|err| err.0)
}

fn refused(bundle: &Value) -> String {
    check(bundle, NAME, Identity::UNIWRT_CI).unwrap_err()
}

/// Flips the last character before any padding of a base64 value.
fn flip(value: &mut Value) {
    let text = value.as_str().unwrap();
    let mut bytes = BASE64.decode(text.as_bytes()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    *value = Value::String(BASE64.encode(&bytes));
}

#[test]
fn a_release_attestation_verifies() {
    let provenance = check(&bundle(), NAME, Identity::UNIWRT_CI).unwrap();
    assert_eq!(
        provenance.commit.as_deref(),
        Some("8e3a0747bd45315fc98e9568f55b80c2b94ff24d")
    );
    assert_eq!(provenance.logged_at, 1_794_857_946);
}

#[test]
fn another_artifact_is_refused() {
    let err = check(
        &bundle(),
        "uniwrt-0.108.0-aarch64-unknown-linux-musl.tar.gz",
        Identity::UNIWRT_CI,
    )
    .unwrap_err();
    assert!(err.contains("doesn't cover"), "{err}");
}

#[test]
fn another_workflow_or_issuer_is_refused() {
    let other_ref = Identity {
        workflow: "https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/dev",
        ..Identity::UNIWRT_CI
    };
    assert!(
        check(&bundle(), NAME, other_ref)
            .unwrap_err()
            .contains("wasn't issued to")
    );
    let other_issuer = Identity {
        issuer: "https://accounts.google.com",
        ..Identity::UNIWRT_CI
    };
    assert!(
        check(&bundle(), NAME, other_issuer)
            .unwrap_err()
            .contains("OIDC issuer")
    );
}

#[test]
fn an_altered_payload_or_signature_is_refused() {
    let mut altered = bundle();
    let payload = altered["dsseEnvelope"]["payload"].as_str().unwrap();
    let mut statement: Value =
        serde_json::from_slice(&BASE64.decode(payload.as_bytes()).unwrap()).unwrap();
    statement["subject"][3]["digest"]["sha256"] = Value::String("0".repeat(64));
    altered["dsseEnvelope"]["payload"] =
        Value::String(BASE64.encode(&serde_json::to_vec(&statement).unwrap()));
    assert!(refused(&altered).contains("another payload"));

    let mut altered = bundle();
    flip(&mut altered["dsseEnvelope"]["signatures"][0]["sig"]);
    assert!(refused(&altered).contains("another signature"));
}

#[test]
fn an_altered_log_entry_is_refused() {
    let mut altered = bundle();
    let entry = &mut altered["verificationMaterial"]["tlogEntries"][0];
    flip(&mut entry["inclusionPromise"]["signedEntryTimestamp"]);
    assert!(refused(&altered).contains("signed entry timestamp"));

    let mut altered = bundle();
    let entry = &mut altered["verificationMaterial"]["tlogEntries"][0];
    entry["integratedTime"] = Value::String("1794857947".to_owned());
    assert!(refused(&altered).contains("signed entry timestamp"));

    let mut altered = bundle();
    let entry = &mut altered["verificationMaterial"]["tlogEntries"][0];
    flip(&mut entry["inclusionProof"]["hashes"][0]);
    assert!(refused(&altered).contains("inclusion proof"));

    let mut altered = bundle();
    let entry = &mut altered["verificationMaterial"]["tlogEntries"][0];
    let checkpoint = entry["inclusionProof"]["checkpoint"]["envelope"]
        .as_str()
        .unwrap()
        .replacen("\n3811253939\n", "\n3811253940\n", 1);
    entry["inclusionProof"]["checkpoint"]["envelope"] = Value::String(checkpoint);
    assert!(refused(&altered).contains("checkpoint is of another tree"));
}

#[test]
fn another_certificate_or_bundle_version_is_refused() {
    let mut altered = bundle();
    flip(&mut altered["verificationMaterial"]["certificate"]["rawBytes"]);
    assert!(refused(&altered).contains("another certificate"));

    let mut altered = bundle();
    altered["mediaType"] = Value::String("application/vnd.dev.sigstore.bundle.v0.4+json".into());
    assert!(refused(&altered).contains("aren't supported"));

    let mut altered = bundle();
    altered["verificationMaterial"]["tlogEntries"][0]["kindVersion"]["version"] =
        Value::String("0.0.2".into());
    assert!(refused(&altered).contains("aren't supported"));
}

#[test]
fn a_certificate_from_a_foreign_ca_is_refused() {
    // Only the CA retired in 2022 kept: the certificate chains to none valid in 2026.
    let mut root: Value =
        serde_json::from_slice(include_bytes!("../../data/sigstore-trusted-root.json")).unwrap();
    root["certificateAuthorities"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let root = TrustedRoot::parse(&serde_json::to_vec(&root).unwrap()).unwrap();
    let digest = sha256();
    let err = verify(
        &serde_json::to_vec(&bundle()).unwrap(),
        Subject {
            name: NAME,
            sha256: &digest,
        },
        Identity::UNIWRT_CI,
        &root,
    )
    .unwrap_err();
    assert!(err.0.contains("no Fulcio CA"), "{}", err.0);
}

/// RFC 6962's tree hash of `leaves`, and the audit path of `index` in it.
fn tree(leaves: &[Vec<u8>]) -> Vec<u8> {
    match leaves.len() {
        1 => leaves[0].clone(),
        n => {
            let split = split(n);
            node_hash(&tree(&leaves[..split]), &tree(&leaves[split..]))
        }
    }
}

fn path(leaves: &[Vec<u8>], index: usize) -> Vec<Vec<u8>> {
    if leaves.len() == 1 {
        return Vec::new();
    }
    let split = split(leaves.len());
    if index < split {
        let mut path = path(&leaves[..split], index);
        path.push(tree(&leaves[split..]));
        path
    } else {
        let mut path = path(&leaves[split..], index - split);
        path.push(tree(&leaves[..split]));
        path
    }
}

/// The largest power of two below `n`.
fn split(n: usize) -> usize {
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}

#[test]
fn inclusion_proofs_hold_for_every_leaf_of_every_small_tree() {
    for size in 1..=17_usize {
        let leaves: Vec<Vec<u8>> = (0..size).map(|i| leaf_hash(&i.to_be_bytes())).collect();
        let root = tree(&leaves);
        for index in 0..size {
            let proof = path(&leaves, index);
            let (i, n) = (index as u64, size as u64);
            assert!(
                included(&leaves[index], i, n, &proof, &root),
                "{index}/{size}"
            );
            let other = (index + 1) % size;
            if other != index {
                assert!(!included(&leaves[other], i, n, &proof, &root));
            }
            assert!(!included(&leaves[index], n, n, &proof, &root));
            if !proof.is_empty() {
                assert!(!included(&leaves[index], i, n, &proof[1..], &root));
            }
        }
    }
}

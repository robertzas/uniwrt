//! The DSSE envelope: its signature over the pre-authentication encoding, and the in-toto
//! statement it carries (the subject's name and SHA-256, the SLSA predicate's commit).

use data_encoding::HEXLOWER;
use serde::Deserialize;

use super::{Subject, Untrusted, cert::Key, untrusted};

const IN_TOTO: &str = "application/vnd.in-toto+json";
const STATEMENT_V1: &str = "https://in-toto.io/Statement/v1";
const SLSA_V1: &str = "https://slsa.dev/provenance/v1";

/// DSSE's pre-authentication encoding: what the signature covers.
pub(super) fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "DSSEv1 {} {payload_type} {} ",
        payload_type.len(),
        payload.len()
    )
    .into_bytes();
    out.extend_from_slice(payload);
    out
}

pub(super) fn verify_signature(
    key: &Key,
    payload_type: &str,
    payload: &[u8],
    signature: &[u8],
) -> Result<(), Untrusted> {
    if key.verifies(&pae(payload_type, payload), signature) {
        Ok(())
    } else {
        Err(untrusted(
            "the envelope's signature doesn't match its certificate",
        ))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Statement {
    #[serde(rename = "_type")]
    kind: String,
    subject: Vec<StatementSubject>,
    predicate_type: String,
    #[serde(default)]
    predicate: Option<Predicate>,
}

#[derive(Deserialize)]
struct StatementSubject {
    name: String,
    digest: Digest,
}

#[derive(Deserialize)]
struct Digest {
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Predicate {
    build_definition: Option<BuildDefinition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildDefinition {
    #[serde(default)]
    resolved_dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dependency {
    #[serde(default)]
    digest: Option<CommitDigest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitDigest {
    #[serde(default)]
    git_commit: Option<String>,
}

/// Checks that the statement names `subject`; returns the commit built, when it tells one.
pub(super) fn check_statement(
    payload_type: &str,
    payload: &[u8],
    subject: Subject<'_>,
) -> Result<Option<String>, Untrusted> {
    if payload_type != IN_TOTO {
        return Err(untrusted(format!(
            "payloads of type {payload_type} aren't supported"
        )));
    }
    let statement: Statement = serde_json::from_slice(payload)
        .map_err(|err| untrusted(format!("the statement is unreadable: {err}")))?;
    if statement.kind != STATEMENT_V1 || statement.predicate_type != SLSA_V1 {
        return Err(untrusted(format!(
            "the statement isn't SLSA provenance v1 ({}, {})",
            statement.kind, statement.predicate_type
        )));
    }
    let digest = HEXLOWER.encode(subject.sha256);
    let named = statement.subject.iter().any(|attested| {
        attested.name == subject.name && attested.digest.sha256.as_deref() == Some(&digest)
    });
    if !named {
        return Err(untrusted(format!(
            "the attestation doesn't cover {} with SHA-256 {digest}",
            subject.name
        )));
    }
    Ok(statement
        .predicate
        .and_then(|predicate| predicate.build_definition)
        .and_then(|build| {
            build
                .resolved_dependencies
                .into_iter()
                .find_map(|dependency| dependency.digest?.git_commit)
        })
        .filter(|commit| commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit())))
}

//! Sigstore attestations of the controller's own releases (D-121, D-123): a minimal verifier of
//! the bundles GitHub's `actions/attest-build-provenance` publishes (bundle v0.3, a DSSE envelope
//! holding an in-toto statement, signed with a Fulcio certificate and logged in Rekor v1).
//!
//! [`verify`] checks, in this order: the bundle's form; the certificate's chain to a Fulcio root
//! of the embedded trust root ([`TrustedRoot::embedded`]) at the time Rekor logged the entry,
//! with the code-signing usage; the certificate's identity (the workflow's URI and the OIDC
//! issuer, [`Identity`]); the envelope's signature by the certificate's key; the statement's
//! subject (the artifact's name and SHA-256); Rekor's entry: its body matching the envelope and
//! the certificate, its signed entry timestamp, and its inclusion proof up to a checkpoint
//! signed by the log. Anything else (Rekor v2 entries, RFC 3161 timestamps, other bundle
//! versions) is refused, never skipped.

mod cert;
mod dsse;
mod rekor;
mod root;
#[cfg(test)]
mod tests;

use data_encoding::BASE64;
use serde::Deserialize;

pub use root::TrustedRoot;

/// Who must have signed: the certificate's SAN URI and its OIDC issuer (Fulcio's extension
/// 1.3.6.1.4.1.57264.1.8).
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    pub workflow: &'a str,
    pub issuer: &'a str,
}

impl Identity<'static> {
    /// uniwrt's CI on `main`, the only workflow that releases it (docs/dev/BUILD.md).
    pub const UNIWRT_CI: Self = Self {
        workflow: "https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/main",
        issuer: "https://token.actions.githubusercontent.com",
    };
}

/// What a verified attestation tells about the build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// The commit built, from the SLSA predicate (absent from other predicates).
    pub commit: Option<String>,
    /// When Rekor logged the attestation, in Unix seconds.
    pub logged_at: i64,
}

/// Why an attestation doesn't hold, in words for the log.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct Untrusted(pub String);

pub(crate) fn untrusted(why: impl Into<String>) -> Untrusted {
    Untrusted(why.into())
}

/// The artifact attested: its file name and SHA-256.
#[derive(Clone, Copy, Debug)]
pub struct Subject<'a> {
    pub name: &'a str,
    pub sha256: &'a [u8; 32],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    media_type: String,
    verification_material: Material,
    dsse_envelope: Option<Envelope>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Material {
    certificate: Option<RawBytes>,
    #[serde(default)]
    tlog_entries: Vec<rekor::Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBytes {
    raw_bytes: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    payload: String,
    payload_type: String,
    signatures: Vec<Signature>,
}

#[derive(Deserialize)]
struct Signature {
    sig: String,
}

const BUNDLE_V03: &str = "application/vnd.dev.sigstore.bundle.v0.3+json";

/// Verifies one Sigstore bundle (its JSON) for `subject`, signed by `identity`.
pub fn verify(
    bundle: &[u8],
    subject: Subject<'_>,
    identity: Identity<'_>,
    root: &TrustedRoot,
) -> Result<Provenance, Untrusted> {
    let bundle: Bundle = serde_json::from_slice(bundle)
        .map_err(|err| untrusted(format!("the bundle is unreadable: {err}")))?;
    if bundle.media_type != BUNDLE_V03 {
        return Err(untrusted(format!(
            "bundles of type {} aren't supported",
            bundle.media_type
        )));
    }
    let leaf = bundle
        .verification_material
        .certificate
        .ok_or_else(|| untrusted("the bundle holds no certificate"))?;
    let leaf = decode(&leaf.raw_bytes, "the certificate")?;
    let envelope = bundle
        .dsse_envelope
        .ok_or_else(|| untrusted("the bundle holds no DSSE envelope"))?;
    let [signature] = envelope.signatures.as_slice() else {
        return Err(untrusted("the envelope must carry exactly one signature"));
    };
    let payload = decode(&envelope.payload, "the payload")?;
    let signature = decode(&signature.sig, "the signature")?;
    let [entry] = bundle.verification_material.tlog_entries.as_slice() else {
        return Err(untrusted("the bundle must carry exactly one Rekor entry"));
    };

    let logged = rekor::verify(
        entry,
        &rekor::Signed {
            payload: &payload,
            signature: &signature,
            certificate: &leaf,
        },
        root,
    )?;
    let leaf = cert::verify(&leaf, logged.integrated_time, identity, root)?;
    dsse::verify_signature(&leaf, &envelope.payload_type, &payload, &signature)?;
    let commit = dsse::check_statement(&envelope.payload_type, &payload, subject)?;
    Ok(Provenance {
        commit,
        logged_at: logged.integrated_time,
    })
}

fn decode(text: &str, what: &str) -> Result<Vec<u8>, Untrusted> {
    BASE64
        .decode(text.as_bytes())
        .map_err(|_| untrusted(format!("{what} isn't base64")))
}

//! Sigstore's trust root (`trusted_root.json` from sigstore/root-signing, embedded from
//! `data/sigstore-trusted-root.json`; `just sigstore-root` refreshes it): Fulcio's certificate
//! chains and Rekor's keys, each with the period it is valid for.

use data_encoding::BASE64;
use serde::Deserialize;
use uniwrt_core::Timestamp;

use super::{Untrusted, untrusted};

const EMBEDDED: &[u8] = include_bytes!("../../data/sigstore-trusted-root.json");
const MEDIA_TYPE: &str = "application/vnd.dev.sigstore.trustedroot+json;version=0.1";

/// The keys and certificates that may vouch for an attestation.
#[derive(Debug)]
pub struct TrustedRoot {
    pub(super) authorities: Vec<Authority>,
    pub(super) logs: Vec<Log>,
}

/// A Fulcio CA: its chain, the intermediates first and the root last (DER).
#[derive(Debug)]
pub(super) struct Authority {
    pub(super) chain: Vec<Vec<u8>>,
    pub(super) valid: Validity,
}

/// A Rekor log: its key ID (SHA-256 of the key's DER), its key (`SubjectPublicKeyInfo` DER).
#[derive(Debug)]
pub(super) struct Log {
    pub(super) key_id: Vec<u8>,
    pub(super) key: Vec<u8>,
    pub(super) ecdsa_p256: bool,
    pub(super) valid: Validity,
}

/// Unix seconds, the end open when absent.
#[derive(Clone, Copy, Debug)]
pub(super) struct Validity {
    start: i64,
    end: Option<i64>,
}

impl Validity {
    pub(super) fn holds_at(self, secs: i64) -> bool {
        self.start <= secs && self.end.is_none_or(|end| secs <= end)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    media_type: String,
    #[serde(default)]
    tlogs: Vec<RawLog>,
    #[serde(default)]
    certificate_authorities: Vec<RawAuthority>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLog {
    public_key: RawKey,
    log_id: RawLogId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawKey {
    raw_bytes: String,
    key_details: String,
    valid_for: RawValidity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLogId {
    key_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAuthority {
    cert_chain: RawChain,
    valid_for: RawValidity,
}

#[derive(Deserialize)]
struct RawChain {
    certificates: Vec<RawCertificate>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCertificate {
    raw_bytes: String,
}

#[derive(Deserialize)]
struct RawValidity {
    start: String,
    #[serde(default)]
    end: Option<String>,
}

impl TrustedRoot {
    /// The trust root built into this binary.
    pub fn embedded() -> Result<Self, Untrusted> {
        Self::parse(EMBEDDED)
    }

    pub fn parse(json: &[u8]) -> Result<Self, Untrusted> {
        let raw: Raw = serde_json::from_slice(json)
            .map_err(|err| untrusted(format!("the trust root is unreadable: {err}")))?;
        if raw.media_type != MEDIA_TYPE {
            return Err(untrusted(format!(
                "trust roots of type {} aren't supported",
                raw.media_type
            )));
        }
        let decode = |text: &str| {
            BASE64
                .decode(text.as_bytes())
                .map_err(|_| untrusted("the trust root holds a value that isn't base64"))
        };
        let authorities = raw
            .certificate_authorities
            .iter()
            .map(|ca| {
                let chain = ca
                    .cert_chain
                    .certificates
                    .iter()
                    .map(|cert| decode(&cert.raw_bytes))
                    .collect::<Result<Vec<_>, _>>()?;
                if chain.is_empty() {
                    return Err(untrusted("the trust root holds an empty certificate chain"));
                }
                // As the file lists it: from the intermediate to the root.
                Ok(Authority {
                    chain,
                    valid: validity(&ca.valid_for)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let logs = raw
            .tlogs
            .iter()
            .map(|log| {
                Ok(Log {
                    key_id: decode(&log.log_id.key_id)?,
                    key: decode(&log.public_key.raw_bytes)?,
                    ecdsa_p256: log.public_key.key_details == "PKIX_ECDSA_P256_SHA_256",
                    valid: validity(&log.public_key.valid_for)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { authorities, logs })
    }
}

fn validity(raw: &RawValidity) -> Result<Validity, Untrusted> {
    let secs = |text: &str| {
        Timestamp::parse_utc(text)
            .map(|at| at.as_unix_ms().div_euclid(1_000))
            .ok_or_else(|| untrusted(format!("the trust root holds a bad time: {text}")))
    };
    Ok(Validity {
        start: secs(&raw.start)?,
        end: raw.end.as_deref().map(secs).transpose()?,
    })
}

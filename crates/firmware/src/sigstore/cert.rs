//! The signing certificate: its chain to one of the trust root's Fulcio CAs (webpki, at the time
//! Rekor logged the entry, for code signing), its identity, and its key.

use std::time::Duration;

use ring::signature::{self, VerificationAlgorithm};
use rustls::pki_types::{CertificateDer, UnixTime};
use webpki::{EndEntityCert, KeyUsage, anchor_from_trusted_cert};
use x509_parser::{extensions::GeneralName, parse_x509_certificate};

use super::{Identity, TrustedRoot, Untrusted, untrusted};

/// id-kp-codeSigning (1.3.6.1.5.5.7.3.3), as webpki takes an OID: its DER value.
const CODE_SIGNING: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];
/// Fulcio's OIDC issuer (v2, a DER `UTF8String`).
const ISSUER_V2: &str = "1.3.6.1.4.1.57264.1.8";

/// The DER prefix of an EC `SubjectPublicKeyInfo` on P-256 and on P-384, before the point.
const SPKI_P256: &[u8] = &[
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a,
    0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];
const SPKI_P384: &[u8] = &[
    0x30, 0x76, 0x30, 0x10, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x05, 0x2b,
    0x81, 0x04, 0x00, 0x22, 0x03, 0x62, 0x00,
];

/// A public key as ring verifies with it: the algorithm and the point.
pub(super) struct Key {
    algorithm: &'static dyn VerificationAlgorithm,
    point: Vec<u8>,
}

impl Key {
    /// An ECDSA key on P-256 or P-384 from its `SubjectPublicKeyInfo` (DER); other keys are refused.
    pub(super) fn from_spki(spki: &[u8]) -> Result<Self, Untrusted> {
        let (algorithm, point): (&'static dyn VerificationAlgorithm, _) =
            if let Some(point) = spki.strip_prefix(SPKI_P256).filter(|p| p.len() == 65) {
                (&signature::ECDSA_P256_SHA256_ASN1, point)
            } else if let Some(point) = spki.strip_prefix(SPKI_P384).filter(|p| p.len() == 97) {
                (&signature::ECDSA_P384_SHA384_ASN1, point)
            } else {
                return Err(untrusted("only ECDSA keys on P-256 or P-384 are supported"));
            };
        Ok(Self {
            algorithm,
            point: point.to_vec(),
        })
    }

    pub(super) fn verifies(&self, message: &[u8], signature: &[u8]) -> bool {
        signature::UnparsedPublicKey::new(self.algorithm, &self.point)
            .verify(message, signature)
            .is_ok()
    }
}

/// Checks the certificate's chain at `at` (Unix seconds) and its identity; returns its key.
pub(super) fn verify(
    der: &[u8],
    at: i64,
    identity: Identity<'_>,
    root: &TrustedRoot,
) -> Result<Key, Untrusted> {
    let leaf = CertificateDer::from(der);
    let end_entity = EndEntityCert::try_from(&leaf)
        .map_err(|err| untrusted(format!("the certificate is malformed: {err}")))?;
    let time = UnixTime::since_unix_epoch(Duration::from_secs(at.max(0).unsigned_abs()));
    let mut chained = false;
    let mut last_error = None;
    for authority in root.authorities.iter().filter(|ca| ca.valid.holds_at(at)) {
        let Some((anchor, intermediates)) = authority.chain.split_last() else {
            continue;
        };
        let anchor = CertificateDer::from(anchor.as_slice());
        let Ok(anchor) = anchor_from_trusted_cert(&anchor) else {
            continue;
        };
        let intermediates: Vec<CertificateDer<'_>> = intermediates
            .iter()
            .map(|cert| CertificateDer::from(cert.as_slice()))
            .collect();
        match end_entity.verify_for_usage(
            webpki::ALL_VERIFICATION_ALGS,
            &[anchor],
            &intermediates,
            time,
            KeyUsage::required(CODE_SIGNING),
            None,
            None,
        ) {
            Ok(_) => {
                chained = true;
                break;
            }
            Err(err) => last_error = Some(err),
        }
    }
    if !chained {
        return Err(untrusted(match last_error {
            Some(err) => format!("the certificate doesn't chain to Sigstore's Fulcio: {err:?}"),
            None => "no Fulcio CA of the trust root was valid when the entry was logged".to_owned(),
        }));
    }

    let (_, cert) = parse_x509_certificate(der)
        .map_err(|err| untrusted(format!("the certificate is malformed: {err}")))?;
    let san = cert
        .subject_alternative_name()
        .map_err(|err| untrusted(format!("the certificate's names are malformed: {err}")))?;
    let signed_by_workflow = san.is_some_and(|san| {
        san.value
            .general_names
            .iter()
            .any(|name| matches!(name, GeneralName::URI(uri) if *uri == identity.workflow))
    });
    if !signed_by_workflow {
        return Err(untrusted(format!(
            "the certificate wasn't issued to {}",
            identity.workflow
        )));
    }
    let issuer = cert
        .extensions()
        .iter()
        .find(|ext| ext.oid.to_id_string() == ISSUER_V2)
        .and_then(|ext| utf8_string(ext.value));
    if issuer != Some(identity.issuer) {
        return Err(untrusted(format!(
            "the certificate's OIDC issuer isn't {}",
            identity.issuer
        )));
    }
    Key::from_spki(cert.public_key().raw)
}

/// A DER `UTF8String`'s text (short or one- or two-byte long form lengths).
fn utf8_string(der: &[u8]) -> Option<&str> {
    let (&tag, rest) = der.split_first()?;
    if tag != 0x0c {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = match first {
        len @ 0..=0x7f => (usize::from(len), rest),
        0x81 => {
            let (&len, rest) = rest.split_first()?;
            (usize::from(len), rest)
        }
        0x82 => {
            let (len, rest) = rest.split_first_chunk::<2>()?;
            (usize::from(u16::from_be_bytes(*len)), rest)
        }
        _ => return None,
    };
    (rest.len() == len).then(|| std::str::from_utf8(rest).ok())?
}

#[cfg(test)]
mod tests {
    use super::utf8_string;

    #[test]
    fn utf8_strings_are_read_in_every_length_form() {
        assert_eq!(utf8_string(b"\x0c\x02hi"), Some("hi"));
        assert_eq!(utf8_string(b"\x0c\x81\x02hi"), Some("hi"));
        assert_eq!(utf8_string(b"\x0c\x82\x00\x02hi"), Some("hi"));
        assert_eq!(utf8_string(b"\x0c\x03hi"), None);
        assert_eq!(utf8_string(b"\x04\x02hi"), None);
        assert_eq!(utf8_string(b""), None);
    }
}

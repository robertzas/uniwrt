//! Issuing the controller's certificates with rcgen (D-039): the local CA ([`Ca`]: ECDSA P-256,
//! 10 years, no intermediates, name-constrained) and the leaf it signs for the HTTPS listener
//! ([`Leaf`]: 397 days, server authentication only). Both are kept in the vault as JSON, with
//! what deciding on a renewal needs, so nothing has to parse X.509.

use std::{fmt::Write as _, net::IpAddr, sync::Arc, time::Duration};

use anyhow::Context as _;
use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, SanType, SerialNumber,
};
use ring::rand::{SecureRandom as _, SystemRandom};
use rustls::{
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _},
    sign::CertifiedKey,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uniwrt_core::Timestamp;

use super::names::{Names, Permitted};

const DAY: Duration = Duration::from_hours(24);
const CA_LIFETIME: Duration = Duration::from_hours(3_652 * 24);
/// Below Apple's limit of 398 days, which its platforms enforce on every TLS server certificate.
const LEAF_LIFETIME: Duration = Duration::from_hours(397 * 24);
/// A leaf is renewed once it has less left than this.
pub(crate) const RENEW_BEFORE: Duration = Duration::from_hours(30 * 24);
/// Certificates start a day early, for devices whose clocks run behind.
const BACKDATE: Duration = DAY;
const ORGANIZATION: &str = "uniwrt";

/// The local CA, as stored in the vault (`tls/ca`).
#[derive(Serialize, Deserialize)]
pub(crate) struct Ca {
    /// The certificate, PEM.
    pub(crate) cert: String,
    /// The private key, PKCS#8 PEM.
    key: String,
    /// The common name, which names this installation's CA in trust stores.
    pub(crate) name: String,
    pub(crate) permitted: Permitted,
    pub(crate) not_after: Timestamp,
}

/// The HTTPS listener's certificate, as stored in the vault (`tls/leaf`).
#[derive(Serialize, Deserialize)]
pub(crate) struct Leaf {
    cert: String,
    key: String,
    pub(crate) names: Names,
    pub(crate) not_after: Timestamp,
    /// The issuing CA's common name: a leaf from another CA is reissued.
    pub(crate) issuer: String,
}

impl Ca {
    pub(crate) fn create(permitted: Permitted, now: Timestamp) -> anyhow::Result<Self> {
        let mut id = [0; 4];
        SystemRandom::new()
            .fill(&mut id)
            .map_err(|_| anyhow::anyhow!("the system's random number generator failed"))?;
        let name = format!("uniwrt local CA {}", hex(&id));
        let key = KeyPair::generate()?;
        let mut params = Self::params(&name, &permitted);
        let not_after = now.saturating_add(CA_LIFETIME);
        params.not_before = datetime(now.saturating_sub(BACKDATE))?;
        params.not_after = datetime(not_after)?;
        params.serial_number = Some(serial()?);
        let cert = params.self_signed(&key)?;
        Ok(Self {
            cert: cert.pem(),
            key: key.serialize_pem(),
            name,
            permitted,
            not_after,
        })
    }

    /// The parameters that make the CA's subject and key identifiers, the same every time.
    fn params(name: &str, permitted: &Permitted) -> CertificateParams {
        let mut params = CertificateParams::default();
        params.distinguished_name = distinguished_name(name);
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let dns = permitted.dns.iter().cloned().map(GeneralSubtree::DnsName);
        let ips = permitted.ips.iter().map(|net| {
            GeneralSubtree::IpAddress(CidrSubnet::from_addr_prefix(net.addr(), net.prefix_len()))
        });
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: dns.chain(ips).collect(),
            excluded_subtrees: Vec::new(),
        });
        params
    }

    /// Signs a new leaf for `names`, which the constraints must cover.
    pub(crate) fn issue(&self, names: &Names, now: Timestamp) -> anyhow::Result<Leaf> {
        let issuer = Issuer::new(
            Self::params(&self.name, &self.permitted),
            KeyPair::from_pem(&self.key).context("the CA's key is unreadable")?,
        );
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        params.distinguished_name = distinguished_name("uniwrt.lan");
        params.subject_alt_names = names
            .dns
            .iter()
            .map(|name| Ok(SanType::DnsName(name.as_str().try_into()?)))
            .chain(
                names
                    .ips
                    .iter()
                    .map(|ip: &IpAddr| Ok(SanType::IpAddress(*ip))),
            )
            .collect::<Result<_, rcgen::Error>>()?;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let not_after = now.saturating_add(LEAF_LIFETIME).min(self.not_after);
        params.not_before = datetime(now.saturating_sub(BACKDATE))?;
        params.not_after = datetime(not_after)?;
        params.serial_number = Some(serial()?);
        let cert = params.signed_by(&key, &issuer)?;
        Ok(Leaf {
            cert: cert.pem(),
            key: key.serialize_pem(),
            names: names.clone(),
            not_after,
            issuer: self.name.clone(),
        })
    }

    pub(crate) fn der(&self) -> anyhow::Result<CertificateDer<'static>> {
        CertificateDer::from_pem_slice(self.cert.as_bytes())
            .context("the CA's certificate is unreadable")
    }
}

impl Leaf {
    /// Why the leaf must be reissued for `names`, if it must.
    pub(crate) fn renewal(
        &self,
        ca_name: &str,
        names: &Names,
        now: Timestamp,
    ) -> Option<&'static str> {
        if self.issuer != ca_name {
            Some("its CA changed")
        } else if &self.names != names {
            Some("the names changed")
        } else if self.not_after.saturating_duration_since(now) < RENEW_BEFORE {
            Some("it expires soon")
        } else {
            None
        }
    }

    /// The chain (leaf, then CA) and key, as rustls serves them.
    pub(crate) fn certified_key(
        &self,
        ca: CertificateDer<'static>,
        provider: &CryptoProvider,
    ) -> anyhow::Result<Arc<CertifiedKey>> {
        let chain = vec![
            CertificateDer::from_pem_slice(self.cert.as_bytes())
                .context("the leaf certificate is unreadable")?,
            ca,
        ];
        let key = PrivateKeyDer::from_pem_slice(self.key.as_bytes())
            .context("the leaf's key is unreadable")?;
        Ok(Arc::new(CertifiedKey::from_der(chain, key, provider)?))
    }
}

fn distinguished_name(common_name: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    name.push(DnType::OrganizationName, ORGANIZATION);
    name
}

/// 128 random bits, positive (RFC 5280 allows up to 20 octets).
fn serial() -> anyhow::Result<SerialNumber> {
    let mut serial = vec![0; 16];
    SystemRandom::new()
        .fill(&mut serial)
        .map_err(|_| anyhow::anyhow!("the system's random number generator failed"))?;
    serial[0] &= 0x7f;
    Ok(SerialNumber::from(serial))
}

fn datetime(at: Timestamp) -> anyhow::Result<OffsetDateTime> {
    Ok(OffsetDateTime::from_unix_timestamp(
        at.as_unix_ms().div_euclid(1_000),
    )?)
}

/// Lowercase hexadecimal.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests;

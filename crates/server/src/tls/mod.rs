//! HTTPS (features/security.md, TLS; D-039): the controller's local CA and the leaf it signs,
//! kept in the vault, served by rustls on the HTTPS listener and renewed while running.
//!
//! [`start`] loads both from the vault, making the CA on the first run and reissuing the leaf
//! when its names, its CA or its expiry call for it; [`Tls`] serves the current leaf to rustls
//! and describes it to the API; [`Renewer`] checks once a day and swaps in a new leaf, so no
//! restart is needed.

mod issue;
pub(crate) mod listener;
pub(crate) mod names;

use std::{
    fmt,
    net::SocketAddr,
    sync::{Arc, PoisonError, RwLock},
    time::Duration,
};

use anyhow::Context as _;
use ring::digest;
use rustls::{
    ServerConfig,
    crypto::{CryptoProvider, ring as provider},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use serde::Serialize;
use tracing::{info, warn};
use uniwrt_api::{Certificate, TlsInfo};
use uniwrt_core::Clock;
use uniwrt_store::{Vault, VaultRepo as _};
use zeroize::Zeroizing;

use issue::{Ca, Leaf};
use names::{Host, Names, Permitted, TlsName};

const CA_SECRET: &str = "tls/ca";
const LEAF_SECRET: &str = "tls/leaf";
const CHECK_EVERY: Duration = Duration::from_hours(24);

/// What the HTTPS listener serves now. Clones share it.
#[derive(Clone)]
pub(crate) struct Tls {
    current: Arc<RwLock<Current>>,
    provider: Arc<CryptoProvider>,
}

/// The CA's key stays in the vault: only a renewal loads it.
struct Current {
    ca_pem: String,
    leaf: Leaf,
    key: Arc<CertifiedKey>,
    info: TlsInfo,
}

/// Renews the leaf while the controller runs; see [`Renewer::run`].
pub(crate) struct Renewer {
    tls: Tls,
    vault: Vault,
    clock: Arc<dyn Clock>,
    configured: Vec<TlsName>,
    listen: Vec<SocketAddr>,
}

/// Loads the CA and leaf from the vault, making or reissuing what is missing or due.
pub(crate) async fn start(
    vault: Vault,
    clock: Arc<dyn Clock>,
    configured: Vec<TlsName>,
    listen: Vec<SocketAddr>,
) -> anyhow::Result<(Tls, Renewer)> {
    let provider = Arc::new(provider::default_provider());
    let wanted = Names::wanted(&configured, &listen, &Host::detect());
    let ca = if let Some(ca) = load::<Ca>(&vault, CA_SECRET).await? {
        ca
    } else {
        let ca = Ca::create(Permitted::for_ca(&wanted), clock.now())?;
        store(&vault, CA_SECRET, &ca).await?;
        info!(ca = %ca.name, "created the controller's local CA; download it from /api/v1/tls/ca.crt to trust it");
        ca
    };
    let names = signable(&ca, &wanted);
    let leaf = match load::<Leaf>(&vault, LEAF_SECRET).await? {
        Some(leaf) if leaf.renewal(&ca.name, &names, clock.now()).is_none() => leaf,
        _ => issue(&vault, &ca, &names, clock.now()).await?,
    };
    let tls = Tls {
        current: Arc::new(RwLock::new(Current::new(&ca, leaf, &provider)?)),
        provider,
    };
    let renewer = Renewer {
        tls: tls.clone(),
        vault,
        clock,
        configured,
        listen,
    };
    Ok((tls, renewer))
}

impl Tls {
    /// rustls' configuration for the HTTPS listener: TLS 1.2 and 1.3, HTTP/2 and HTTP/1.1.
    pub(crate) fn server_config(&self) -> anyhow::Result<Arc<ServerConfig>> {
        let mut config = ServerConfig::builder_with_provider(Arc::clone(&self.provider))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(self.clone()));
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    fn read<T>(&self, f: impl FnOnce(&Current) -> T) -> T {
        f(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// A CA and leaf made in memory, for tests of what serves them.
    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        let provider = Arc::new(provider::default_provider());
        let now = uniwrt_core::SystemClock.now();
        let names = Names::wanted(&[], &[], &Host::default());
        let ca = Ca::create(Permitted::local(), now).unwrap();
        let leaf = ca.issue(&names, now).unwrap();
        Self {
            current: Arc::new(RwLock::new(Current::new(&ca, leaf, &provider).unwrap())),
            provider,
        }
    }
}

impl Certificate for Tls {
    fn info(&self) -> TlsInfo {
        self.read(|current| current.info.clone())
    }

    fn ca_pem(&self) -> String {
        self.read(|current| current.ca_pem.clone())
    }
}

impl ResolvesServerCert for Tls {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.read(|current| Arc::clone(&current.key)))
    }
}

impl fmt::Debug for Tls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tls")
            .field("info", &self.info())
            .finish_non_exhaustive()
    }
}

impl Renewer {
    /// Checks the leaf once a day for as long as it runs; abort the task to stop it.
    pub(crate) async fn run(self) {
        loop {
            tokio::time::sleep(CHECK_EVERY).await;
            if let Err(err) = self.check().await {
                warn!("cannot renew the HTTPS certificate: {err:#}");
            }
        }
    }

    /// Reissues the leaf if it is due; true when it did.
    async fn check(&self) -> anyhow::Result<bool> {
        let ca = load::<Ca>(&self.vault, CA_SECRET)
            .await?
            .context("the vault has lost the local CA")?;
        let names = signable(
            &ca,
            &Names::wanted(&self.configured, &self.listen, &Host::detect()),
        );
        let now = self.clock.now();
        let Some(why) = self
            .tls
            .read(|current| current.leaf.renewal(&ca.name, &names, now))
        else {
            return Ok(false);
        };
        info!("renewing the HTTPS certificate: {why}");
        let leaf = issue(&self.vault, &ca, &names, now).await?;
        let next = Current::new(&ca, leaf, &self.tls.provider)?;
        *self
            .tls
            .current
            .write()
            .unwrap_or_else(PoisonError::into_inner) = next;
        Ok(true)
    }
}

impl Current {
    fn new(ca: &Ca, leaf: Leaf, provider: &CryptoProvider) -> anyhow::Result<Self> {
        let der = ca.der()?;
        let fingerprint = digest::digest(&digest::SHA256, &der);
        let key = leaf.certified_key(der, provider)?;
        let ca_sha256 = fingerprint
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        let names = leaf
            .names
            .dns
            .iter()
            .cloned()
            .chain(leaf.names.ips.iter().map(ToString::to_string))
            .collect();
        let info = TlsInfo {
            ca_name: ca.name.clone(),
            ca_sha256,
            ca_not_after: ca.not_after.to_string(),
            names,
            not_after: leaf.not_after.to_string(),
        };
        Ok(Self {
            ca_pem: ca.cert.clone(),
            leaf,
            key,
            info,
        })
    }
}

/// The wanted names the CA may sign; the rest are left out, with a warning.
fn signable(ca: &Ca, wanted: &Names) -> Names {
    let (signable, outside) = ca.permitted.split(wanted);
    if outside != Names::default() {
        warn!(ca = %ca.name, "the HTTPS certificate cannot cover {outside}: the local CA was made before these names were configured, and may not sign them");
    }
    signable
}

async fn issue(
    vault: &Vault,
    ca: &Ca,
    names: &Names,
    now: uniwrt_core::Timestamp,
) -> anyhow::Result<Leaf> {
    let leaf = ca.issue(names, now)?;
    store(vault, LEAF_SECRET, &leaf).await?;
    info!(names = %leaf.names, not_after = %leaf.not_after, "issued the HTTPS certificate");
    Ok(leaf)
}

async fn load<T: serde::de::DeserializeOwned>(
    vault: &Vault,
    name: &str,
) -> anyhow::Result<Option<T>> {
    let Some(json) = vault.secret(name).await? else {
        return Ok(None);
    };
    serde_json::from_slice(&json)
        .map(Some)
        .with_context(|| format!("the vault's {name} is unreadable"))
}

async fn store(vault: &Vault, name: &str, value: &impl Serialize) -> anyhow::Result<()> {
    let json = Zeroizing::new(serde_json::to_vec(value)?);
    vault
        .put_secret(name, json)
        .await
        .with_context(|| format!("cannot store {name} in the vault"))
}

#[cfg(test)]
mod tests;

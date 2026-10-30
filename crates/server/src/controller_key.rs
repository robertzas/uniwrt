//! The controller's SSH key: generated on the first run and kept in the vault
//! (features/adoption.md, Identity, credentials, trust).

use anyhow::Context as _;
use tracing::info;
use uniwrt_node::ControllerKey;
use uniwrt_store::VaultRepo;
use zeroize::Zeroizing;

/// The vault entry holding the key as OpenSSH text.
const SECRET: &str = "controller/ssh_key";

/// Loads the controller key, generating and storing it on the first run.
pub(crate) async fn load_or_generate(vault: &impl VaultRepo) -> anyhow::Result<ControllerKey> {
    if let Some(pem) = vault
        .secret(SECRET)
        .await
        .context("cannot read the controller key")?
    {
        return ControllerKey::from_openssh(&pem)
            .context("the controller key in the vault is unreadable");
    }
    let key = ControllerKey::generate().context("cannot generate the controller key")?;
    let pem = key.to_openssh()?;
    vault
        .put_secret(SECRET, Zeroizing::new(pem.as_bytes().to_vec()))
        .await
        .context("cannot store the controller key")?;
    info!(fingerprint = %key.fingerprint(), "generated the controller's SSH key");
    Ok(key)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uniwrt_core::SystemClock;
    use uniwrt_store::{MasterKey, Store, Vault};

    use super::load_or_generate;

    #[tokio::test]
    async fn the_key_is_generated_once_and_then_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("uniwrt.db");
        let key = MasterKey::generate().unwrap();
        let store = Store::open(&path, Arc::new(SystemClock)).await.unwrap();
        let first = load_or_generate(&Vault::open(&store, &key).await.unwrap())
            .await
            .unwrap();
        store.close().await;

        let store = Store::open(&path, Arc::new(SystemClock)).await.unwrap();
        let again = load_or_generate(&Vault::open(&store, &key).await.unwrap())
            .await
            .unwrap();
        assert_eq!(again.fingerprint(), first.fingerprint());
        store.close().await;
    }
}

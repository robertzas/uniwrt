//! The controller's roaming key (features/wifi.md, Roaming; D-085): 32 random bytes made on
//! first use and kept in the vault (`controller/roaming_key`), from which each WLAN's 802.11r
//! key is derived. Its access points protect the keys they hand each other under SAE with it,
//! so a client that knows the passphrase can't read them (OpenWrt's own default derives the key
//! from the passphrase).

use std::fmt::Write as _;

use ring::{
    hmac,
    rand::{SecureRandom as _, SystemRandom},
};
use rusqlite::params;
use zeroize::Zeroizing;

use super::{ALG, Vault};
use crate::{Error, Result, VaultRepo as _};

/// Its name in the vault.
const NAME: &str = "controller/roaming_key";
const LEN: usize = 32;

/// The controller's roaming key.
pub struct RoamingKey(hmac::Key);

impl RoamingKey {
    /// The 802.11r key of the WLAN `wlan` while its passphrase is the vault's `passphrase`
    /// (its name there): 64 hex digits, HMAC-SHA256 over both, so a new passphrase brings a new
    /// key.
    pub fn ft_key(&self, wlan: &str, passphrase: &str) -> Zeroizing<String> {
        let mut context = hmac::Context::with_key(&self.0);
        for part in [
            b"uniwrt 802.11r v1".as_slice(),
            wlan.as_bytes(),
            passphrase.as_bytes(),
        ] {
            context.update(part);
            context.update(&[0]);
        }
        let mut text = Zeroizing::new(String::with_capacity(2 * LEN));
        for byte in context.sign().as_ref() {
            // Writing to a String can't fail.
            let _ = write!(text, "{byte:02x}");
        }
        text
    }
}

impl Vault {
    /// The controller's roaming key, made and stored the first time it is asked for. The insert
    /// keeps a key already there, so callers racing to make it all get the stored one.
    pub async fn roaming_key(&self) -> Result<RoamingKey> {
        if let Some(stored) = self.secret(NAME).await? {
            return key(&stored);
        }
        let mut fresh = Zeroizing::new([0_u8; LEN]);
        SystemRandom::new()
            .fill(fresh.as_mut())
            .map_err(|_| Error::Random)?;
        let sealed = self.sealer.seal(NAME, fresh.as_ref())?;
        let now = self.store.now().as_unix_ms();
        self.store
            .write(move |conn| {
                conn.prepare_cached(
                    "INSERT INTO vault (name, alg, value, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT (name) DO NOTHING",
                )?
                .execute(params![NAME, ALG, sealed, now])?;
                Ok(())
            })
            .await?;
        let stored = self.secret(NAME).await?.ok_or(Error::Unseal {
            name: NAME.to_owned(),
        })?;
        key(&stored)
    }
}

fn key(stored: &[u8]) -> Result<RoamingKey> {
    if stored.len() != LEN {
        return Err(Error::Unseal {
            name: NAME.to_owned(),
        });
    }
    Ok(RoamingKey(hmac::Key::new(hmac::HMAC_SHA256, stored)))
}

#[cfg(test)]
mod tests {
    use crate::{MasterKey, Vault, VaultRepo as _, testing};

    use super::NAME;

    #[tokio::test]
    async fn the_key_is_made_once_and_derives_a_key_per_wlan_and_passphrase() {
        let (_dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let (first, again) = tokio::join!(vault.roaming_key(), vault.roaming_key());
        let (first, again) = (first.unwrap(), again.unwrap());
        let key = first.ft_key("home", "config/01");
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(*key, *again.ft_key("home", "config/01"));
        assert_eq!(
            *key,
            *vault
                .roaming_key()
                .await
                .unwrap()
                .ft_key("home", "config/01")
        );
        assert_ne!(*key, *first.ft_key("home", "config/02"));
        assert_ne!(*key, *first.ft_key("guest", "config/01"));
        assert_eq!(vault.secret(NAME).await.unwrap().unwrap().len(), 32);
    }
}

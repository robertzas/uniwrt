//! Sealing one secret: XChaCha20-Poly1305 under a subkey of the master key, with a random nonce
//! and the secret's name as associated data, so a sealed value cannot be moved to another name.

use chacha20poly1305::{
    KeyInit as _, XChaCha20Poly1305, XNonce,
    aead::{Aead as _, Payload},
};
use ring::{
    hkdf,
    rand::{SecureRandom as _, SystemRandom},
};
use zeroize::Zeroizing;

use super::MasterKey;
use crate::{Error, Result, Secret};

/// The `alg` of sealed rows: `value` is the nonce followed by the ciphertext and its tag.
pub(crate) const ALG: &str = "xchacha20poly1305";
const NONCE_LEN: usize = 24;
/// Derives the vault's key from the master key, which may key other things later (backups).
const SUBKEY_INFO: &[u8] = b"uniwrt vault v1";

pub(crate) struct Sealer {
    cipher: XChaCha20Poly1305,
    random: SystemRandom,
}

impl Sealer {
    pub(crate) fn new(master: &MasterKey) -> Self {
        struct Len32;
        impl hkdf::KeyType for Len32 {
            fn len(&self) -> usize {
                32
            }
        }
        let mut subkey = Zeroizing::new([0; 32]);
        // Cannot fail: 32 bytes is far below HKDF-SHA256's limit of 255 × 32.
        hkdf::Salt::new(hkdf::HKDF_SHA256, &[])
            .extract(master.bytes())
            .expand(&[SUBKEY_INFO], Len32)
            .and_then(|okm| okm.fill(subkey.as_mut()))
            .expect("HKDF output of 32 bytes");
        Self {
            cipher: XChaCha20Poly1305::new(&(*subkey).into()),
            random: SystemRandom::new(),
        }
    }

    pub(crate) fn seal(&self, name: &str, secret: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0; NONCE_LEN];
        self.random.fill(&mut nonce).map_err(|_| Error::Random)?;
        let payload = Payload {
            msg: secret,
            aad: name.as_bytes(),
        };
        let sealed = self
            .cipher
            .encrypt(&XNonce::from(nonce), payload)
            .map_err(|_| Error::Seal {
                name: name.to_owned(),
            })?;
        let mut value = Vec::with_capacity(NONCE_LEN + sealed.len());
        value.extend_from_slice(&nonce);
        value.extend_from_slice(&sealed);
        Ok(value)
    }

    /// Opens a sealed value; `None` when it was sealed under another key, for another name, or
    /// altered.
    pub(crate) fn open(&self, name: &str, value: &[u8]) -> Option<Secret> {
        let (nonce, sealed) = value.split_at_checked(NONCE_LEN)?;
        let nonce = XNonce::try_from(nonce).ok()?;
        let payload = Payload {
            msg: sealed,
            aad: name.as_bytes(),
        };
        self.cipher
            .decrypt(&nonce, payload)
            .ok()
            .map(Zeroizing::new)
    }
}

#[cfg(test)]
mod tests {
    use super::{NONCE_LEN, Sealer};
    use crate::MasterKey;

    #[test]
    fn sealed_values_open_only_under_their_key_and_name() {
        let key = MasterKey::generate().unwrap();
        let sealer = Sealer::new(&key);
        let value = sealer.seal("a", b"secret").unwrap();
        assert_eq!(value.len(), NONCE_LEN + 6 + 16);
        assert_eq!(
            sealer.open("a", &value).as_deref(),
            Some(&b"secret".to_vec())
        );
        // The nonce is fresh each time.
        assert_ne!(sealer.seal("a", b"secret").unwrap(), value);

        assert_eq!(sealer.open("b", &value), None);
        let other = Sealer::new(&MasterKey::generate().unwrap());
        assert_eq!(other.open("a", &value), None);
        let mut altered = value.clone();
        *altered.last_mut().unwrap() ^= 1;
        assert_eq!(sealer.open("a", &altered), None);
        assert_eq!(sealer.open("a", &value[..NONCE_LEN]), None);
        assert_eq!(sealer.open("a", &value[..3]), None);
    }

    #[test]
    fn the_same_master_key_derives_the_same_vault_key() {
        let key = MasterKey::generate().unwrap();
        let value = Sealer::new(&key).seal("a", b"").unwrap();
        let again = MasterKey::parse(&key.to_text()).unwrap();
        assert_eq!(
            Sealer::new(&again).open("a", &value).as_deref(),
            Some(&Vec::new())
        );
    }
}

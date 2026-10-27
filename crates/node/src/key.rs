//! The controller's SSH identity: one ed25519 key that every adopted node trusts.

use std::{fmt, sync::Arc};

use ring::rand::{SecureRandom as _, SystemRandom};
use russh::keys::{
    Algorithm, HashAlg, PrivateKey, PrivateKeyWithHashAlg,
    ssh_key::{LineEnding, private::Ed25519Keypair},
};
use zeroize::Zeroizing;

use crate::{Error, Result};

/// The comment that marks the controller's line in a node's `authorized_keys`.
pub const AUTHORIZED_KEY_COMMENT: &str = "uniwrt-controller";

/// The controller's private key. It is kept in the vault as OpenSSH text
/// ([`to_openssh`](Self::to_openssh)); `Debug` shows only the fingerprint.
#[derive(Clone)]
pub struct ControllerKey(Arc<PrivateKey>);

impl ControllerKey {
    /// A new random key, commented [`AUTHORIZED_KEY_COMMENT`].
    pub fn generate() -> Result<Self> {
        let mut seed = Zeroizing::new([0_u8; 32]);
        SystemRandom::new()
            .fill(seed.as_mut())
            .map_err(|_| Error::Random)?;
        let mut key = PrivateKey::from(Ed25519Keypair::from_seed(&seed));
        key.set_comment(AUTHORIZED_KEY_COMMENT);
        Ok(Self(Arc::new(key)))
    }

    /// Reads an OpenSSH private key (`-----BEGIN OPENSSH PRIVATE KEY-----`), which must be an
    /// unencrypted ed25519 key.
    pub fn from_openssh(pem: &[u8]) -> Result<Self> {
        const WHAT: &str = "controller key";
        let key =
            PrivateKey::from_openssh(pem).map_err(|source| Error::Key { what: WHAT, source })?;
        if key.algorithm() != Algorithm::Ed25519 || key.is_encrypted() {
            return Err(Error::KeyType {
                what: WHAT,
                found: if key.is_encrypted() {
                    "an encrypted key".to_owned()
                } else {
                    key.algorithm().to_string()
                },
            });
        }
        Ok(Self(Arc::new(key)))
    }

    pub fn to_openssh(&self) -> Result<Zeroizing<String>> {
        self.0
            .to_openssh(LineEnding::LF)
            .map_err(|source| Error::Key {
                what: "controller key",
                source,
            })
    }

    /// The line for a node's `authorized_keys`: `ssh-ed25519 AAAA… uniwrt-controller`.
    pub fn authorized_key(&self) -> Result<String> {
        self.0
            .public_key()
            .to_openssh()
            .map_err(|source| Error::Key {
                what: "controller public key",
                source,
            })
    }

    /// SHA-256 fingerprint, as OpenSSH shows it.
    pub fn fingerprint(&self) -> String {
        self.0.fingerprint(HashAlg::Sha256).to_string()
    }

    pub(crate) fn credential(&self) -> PrivateKeyWithHashAlg {
        PrivateKeyWithHashAlg::new(Arc::clone(&self.0), None)
    }
}

impl fmt::Debug for ControllerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ControllerKey({})", self.fingerprint())
    }
}

#[cfg(test)]
mod tests {
    use super::{AUTHORIZED_KEY_COMMENT, ControllerKey};
    use crate::Error;

    #[test]
    fn a_generated_key_round_trips_through_openssh_text() {
        let key = ControllerKey::generate().unwrap();
        let pem = key.to_openssh().unwrap();
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
        let back = ControllerKey::from_openssh(pem.as_bytes()).unwrap();
        assert_eq!(back.fingerprint(), key.fingerprint());
        assert_eq!(
            back.authorized_key().unwrap(),
            key.authorized_key().unwrap()
        );

        let line = key.authorized_key().unwrap();
        assert!(line.starts_with("ssh-ed25519 AAAA"), "{line}");
        assert!(
            line.ends_with(&format!(" {AUTHORIZED_KEY_COMMENT}")),
            "{line}"
        );
        assert!(key.fingerprint().starts_with("SHA256:"));
        assert_eq!(
            format!("{key:?}"),
            format!("ControllerKey({})", key.fingerprint())
        );
    }

    #[test]
    fn keys_are_random() {
        let a = ControllerKey::generate().unwrap();
        let b = ControllerKey::generate().unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn garbage_is_refused() {
        let err = ControllerKey::from_openssh(b"not a key").unwrap_err();
        assert!(matches!(err, Error::Key { .. }), "{err:?}");
    }
}

//! Configuration secrets (features/security.md, Secrets vault): Wi-Fi passphrases, the mesh key,
//! the PPPoE password. Each value is sealed under a new random name (`config/<32 hex digits>`)
//! that the site's document refers to, and committed with the revision that introduces it
//! ([`RevisionRepo::commit_revision`](crate::RevisionRepo::commit_revision)), so that an older
//! revision keeps finding the value it had.

use std::fmt::{self, Write as _};

use uniwrt_model::site::SecretRef;

use super::Vault;
use crate::{Error, Result};

/// A configuration secret sealed under the master key, ready to commit with a revision.
pub struct SealedSecret {
    pub(crate) name: SecretRef,
    pub(crate) value: Vec<u8>,
}

impl SealedSecret {
    /// The reference the document holds.
    pub fn reference(&self) -> &SecretRef {
        &self.name
    }
}

impl fmt::Debug for SealedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SealedSecret")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Seals `secret` under a new name.
    pub fn seal_config_secret(&self, secret: &[u8]) -> Result<SealedSecret> {
        let mut id = [0; 16];
        self.sealer.random(&mut id)?;
        let name = id
            .iter()
            .fold(String::from(SecretRef::PREFIX), |mut name, byte| {
                let _ = write!(name, "{byte:02x}");
                name
            });
        let name = SecretRef::new(name).ok_or(Error::Random)?;
        Ok(SealedSecret {
            value: self.sealer.seal(name.name(), secret)?,
            name,
        })
    }
}

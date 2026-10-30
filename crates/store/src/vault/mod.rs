//! The secrets vault ([`Vault`]): [`VaultRepo`] over the `vault` table, every value sealed with
//! XChaCha20-Poly1305 under the [`MasterKey`] (features/security.md, Secrets vault).
//!
//! Opening the vault checks the key against every sealed row, so a wrong key fails at startup
//! rather than at the first secret read, and reseals the rows that the M1.2 stub stored
//! unencrypted (`alg = 'plain'`, D-032).

mod backup;
mod key;
mod seal;

use std::{fmt, sync::Arc};

use rusqlite::{OptionalExtension as _, params};
use tracing::info;

pub use backup::SealedBackup;
pub(crate) use backup::backup_from_row;
pub use key::MasterKey;
pub(crate) use seal::ALG;
use seal::Sealer;

use crate::{Error, Result, Secret, Store, VaultRepo};

/// How the M1.2 stub stored a value: unencrypted.
const PLAIN: &str = "plain";

/// The vault: secrets sealed under the master key. Clones share it.
#[derive(Clone)]
pub struct Vault {
    store: Store,
    sealer: Arc<Sealer>,
}

impl Vault {
    /// Opens the vault with `key`: fails with [`Error::WrongMasterKey`] when any sealed secret
    /// does not open with it, and seals the rows stored unencrypted before M1.6.
    pub async fn open(store: &Store, key: &MasterKey) -> Result<Self> {
        let sealer = Arc::new(Sealer::new(key));
        let resealed = store
            .write({
                let sealer = Arc::clone(&sealer);
                move |conn| {
                    let rows = conn
                        .prepare("SELECT name, alg, value FROM vault")?
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get(2)?,
                            ))
                        })?
                        .collect::<rusqlite::Result<Vec<(String, String, Vec<u8>)>>>()?;
                    let total = rows.len();
                    let (mut unreadable, mut resealed) = (0, 0);
                    for (name, alg, value) in rows {
                        match alg.as_str() {
                            ALG if sealer.open(&name, &value).is_none() => unreadable += 1,
                            ALG => {}
                            PLAIN => {
                                let plain = zeroize::Zeroizing::new(value);
                                conn.prepare_cached(
                                    "UPDATE vault SET alg = ?2, value = ?3 WHERE name = ?1",
                                )?
                                .execute(params![
                                    name,
                                    ALG,
                                    sealer.seal(&name, &plain)?
                                ])?;
                                resealed += 1;
                            }
                            _ => return Err(Error::UnknownSeal { name, alg }),
                        }
                    }
                    if unreadable > 0 {
                        // Leaves the plain rows alone: the batch's savepoint rolls back.
                        return Err(Error::WrongMasterKey { unreadable, total });
                    }
                    Ok(resealed)
                }
            })
            .await?;
        if resealed > 0 {
            info!(resealed, "sealed the secrets stored unencrypted before");
        }
        Ok(Self {
            store: store.clone(),
            sealer,
        })
    }
}

impl Store {
    /// Whether the vault holds sealed secrets, which only their master key opens: a missing
    /// key must then not be replaced by a new one.
    pub async fn has_sealed_secrets(&self) -> Result<bool> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached("SELECT EXISTS (SELECT 1 FROM vault WHERE alg <> ?1)")?
                .query_row([PLAIN], |row| row.get(0))?)
        })
        .await
    }
}

impl VaultRepo for Vault {
    async fn put_secret(&self, name: &str, value: Secret) -> Result<()> {
        let sealed = self.sealer.seal(name, &value)?;
        let name = name.to_owned();
        let now = self.store.now().as_unix_ms();
        self.store
            .write(move |conn| {
                conn.prepare_cached(
                    "INSERT INTO vault (name, alg, value, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)
                     ON CONFLICT (name) DO UPDATE
                     SET alg = excluded.alg, value = excluded.value,
                         updated_at = excluded.updated_at",
                )?
                .execute(params![name, ALG, sealed, now])?;
                Ok(())
            })
            .await
    }

    async fn secret(&self, name: &str) -> Result<Option<Secret>> {
        let name = name.to_owned();
        let sealer = Arc::clone(&self.sealer);
        self.store
            .read(move |conn| {
                let Some((alg, value)) = conn
                    .prepare_cached("SELECT alg, value FROM vault WHERE name = ?1")?
                    .query_row([&name], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                    })
                    .optional()?
                else {
                    return Ok(None);
                };
                if alg != ALG {
                    return Err(Error::UnknownSeal { name, alg });
                }
                sealer
                    .open(&name, &value)
                    .map(Some)
                    .ok_or(Error::Unseal { name })
            })
            .await
    }

    async fn delete_secret(&self, name: &str) -> Result<bool> {
        let name = name.to_owned();
        self.store
            .write(move |conn| {
                Ok(conn
                    .prepare_cached("DELETE FROM vault WHERE name = ?1")?
                    .execute([name])?
                    > 0)
            })
            .await
    }
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;

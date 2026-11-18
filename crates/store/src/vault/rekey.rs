//! Resealing the whole vault under another master key: a restored database on a host that keeps
//! its own key (`UNIWRT_MASTER_KEY`, a Docker secret; D-129).

use std::sync::Arc;

use rusqlite::params;

use super::{ALG, MasterKey, Sealer, Vault, backup::seal_name};
use crate::{Error, Result};

impl Vault {
    /// Reseals every secret and node backup under `key` in one transaction, and returns the vault
    /// that uses it. Fails, changing nothing, if a value does not open under the current key.
    pub async fn rekey(self, key: &MasterKey) -> Result<Self> {
        let new = Arc::new(Sealer::new(key));
        let old = Arc::clone(&self.sealer);
        self.store
            .write({
                let new = Arc::clone(&new);
                move |conn| {
                    let secrets = conn
                        .prepare("SELECT name, alg, value FROM vault")?
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, Vec<u8>>(2)?,
                            ))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    for (name, alg, value) in secrets {
                        let value = reseal(&old, &new, &name, &alg, &value)?;
                        conn.prepare_cached("UPDATE vault SET value = ?2 WHERE name = ?1")?
                            .execute(params![name, value])?;
                    }
                    let backups = conn
                        .prepare("SELECT id, sha256, alg, value FROM node_backups")?
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Vec<u8>>(3)?,
                            ))
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    for (id, sha256, alg, value) in backups {
                        let value = reseal(&old, &new, &seal_name(&sha256), &alg, &value)?;
                        conn.prepare_cached("UPDATE node_backups SET value = ?2 WHERE id = ?1")?
                            .execute(params![id, value])?;
                    }
                    Ok(())
                }
            })
            .await?;
        Ok(Self {
            store: self.store,
            sealer: new,
            key: Arc::new(key.duplicate()),
        })
    }
}

fn reseal(old: &Sealer, new: &Sealer, name: &str, alg: &str, value: &[u8]) -> Result<Vec<u8>> {
    if alg != ALG {
        return Err(Error::UnknownSeal {
            name: name.to_owned(),
            alg: alg.to_owned(),
        });
    }
    let plain = old.open(name, value).ok_or_else(|| Error::Unseal {
        name: name.to_owned(),
    })?;
    new.seal(name, &plain)
}

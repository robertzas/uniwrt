//! Configuration backups, sealed like secrets: [`Vault::seal_backup`] before they are stored
//! (with the node at adoption, [`Adoption`](crate::Adoption)), [`Vault::open_backup`] to read one
//! back.

use std::{fmt::Write as _, sync::Arc};

use ring::digest;
use rusqlite::OptionalExtension as _;
use uniwrt_core::{BackupId, NodeId, Timestamp};
use uniwrt_model::inventory::{Backup, BackupReason};

use super::{ALG, Vault};
use crate::{
    Error, Result, Secret,
    codec::{enum_column, timestamp_column},
};

/// A backup sealed under the master key, ready to store.
pub struct SealedBackup {
    pub(crate) taken_at: Timestamp,
    pub(crate) reason: BackupReason,
    pub(crate) size: i64,
    pub(crate) sha256: String,
    pub(crate) value: Vec<u8>,
}

impl SealedBackup {
    /// SHA-256 of the tarball, in hexadecimal.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// The associated data a backup is sealed with: its digest, so a sealed tarball cannot pass for
/// another.
fn seal_name(sha256: &str) -> String {
    format!("backup/{sha256}")
}

impl Vault {
    pub fn seal_backup(
        &self,
        reason: BackupReason,
        taken_at: Timestamp,
        tarball: &[u8],
    ) -> Result<SealedBackup> {
        let sha256 = digest::digest(&digest::SHA256, tarball)
            .as_ref()
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        Ok(SealedBackup {
            taken_at,
            reason,
            size: i64::try_from(tarball.len()).unwrap_or(i64::MAX),
            value: self.sealer.seal(&seal_name(&sha256), tarball)?,
            sha256,
        })
    }

    /// A stored backup and its tarball.
    pub async fn open_backup(&self, id: BackupId) -> Result<Option<(Backup, Secret)>> {
        let sealer = Arc::clone(&self.sealer);
        self.store
            .read(move |conn| {
                let Some((backup, alg, value)) = conn
                    .prepare_cached(
                        "SELECT id, node_id, taken_at, reason, size, sha256, alg, value
                         FROM node_backups WHERE id = ?1",
                    )?
                    .query_row([id.get()], |row| {
                        Ok((
                            backup_from_row(row)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Vec<u8>>(7)?,
                        ))
                    })
                    .optional()?
                else {
                    return Ok(None);
                };
                let name = seal_name(&backup.sha256);
                if alg != ALG {
                    return Err(Error::UnknownSeal { name, alg });
                }
                let tarball = sealer.open(&name, &value).ok_or(Error::Unseal { name })?;
                Ok(Some((backup, tarball)))
            })
            .await
    }
}

/// A backup's description from the first six columns: `id`, `node_id`, `taken_at`, `reason`,
/// `size` and `sha256`.
pub(crate) fn backup_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Backup> {
    Ok(Backup {
        id: BackupId::new(row.get(0)?),
        node: NodeId::new(row.get(1)?),
        taken_at: timestamp_column(row, 2)?,
        reason: enum_column(row, 3)?,
        size_bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or_default(),
        sha256: row.get(5)?,
    })
}

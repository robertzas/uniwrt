//! [`BackupRepo`] on SQLite: the `node_backups` table kept by count and content (features/
//! lifecycle.md, Backups; D-126). Adoption writes a node's first backup with the node
//! ([`Adoption`](crate::Adoption)).

use std::{collections::BTreeMap, future::Future};

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::{BackupId, NodeId, Timestamp};
use uniwrt_model::inventory::{BACKUPS_KEPT, Backup, BackupReason};

use super::adoption::insert_backup;
use crate::{
    Result, SealedBackup, Store,
    codec::enum_text,
    vault::{BACKUP_COLUMNS, backup_from_row},
};

/// What [`BackupRepo::keep_backup`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Stored as a new backup.
    New(BackupId),
    /// The node's newest backup holds the same files: only its `seen_at` moved.
    Unchanged(Backup),
}

/// The nodes' configuration backups.
pub trait BackupRepo: Send + Sync {
    /// Keeps a sealed backup of `node` (D-126). One holding the same files as the node's newest
    /// (unless that is the adoption's) only marks that one seen at its time; otherwise it is
    /// stored, and the node's oldest are deleted beyond the newest [`BACKUPS_KEPT`] (the
    /// adoption's is always kept besides).
    fn keep_backup(
        &self,
        node: NodeId,
        backup: SealedBackup,
    ) -> impl Future<Output = Result<Kept>> + Send;

    /// The node's configuration backups, newest first;
    /// [`Vault::open_backup`](crate::Vault::open_backup) reads one.
    fn backups(&self, node: NodeId) -> impl Future<Output = Result<Vec<Backup>>> + Send;

    /// When each node with backups was last found backed up: its newest `seen_at`.
    fn backups_seen(&self) -> impl Future<Output = Result<BTreeMap<NodeId, Timestamp>>> + Send;
}

/// Newest first: by when each was last seen, then by id.
const NEWEST: &str = "ORDER BY coalesce(seen_at, taken_at) DESC, id DESC";

impl BackupRepo for Store {
    async fn keep_backup(&self, node: NodeId, backup: SealedBackup) -> Result<Kept> {
        // A write runs in its own savepoint: all or nothing.
        self.write(move |conn| {
            let newest = conn
                .prepare_cached(&format!(
                    "SELECT {BACKUP_COLUMNS}, content FROM node_backups WHERE node_id = ?1
                     {NEWEST} LIMIT 1"
                ))?
                .query_row([node.get()], |row| {
                    Ok((backup_from_row(row)?, row.get::<_, Option<String>>(7)?))
                })
                .optional()?;
            if let Some((mut newest, Some(content))) = newest
                && !newest.reason.pinned()
                && backup.content.as_ref() == Some(&content)
            {
                conn.prepare_cached("UPDATE node_backups SET seen_at = ?2 WHERE id = ?1")?
                    .execute(params![newest.id.get(), backup.taken_at.as_unix_ms()])?;
                newest.seen_at = backup.taken_at;
                return Ok(Kept::Unchanged(newest));
            }
            insert_backup(conn, node, &backup)?;
            let id = BackupId::new(conn.last_insert_rowid());
            conn.prepare_cached(&format!(
                "DELETE FROM node_backups WHERE node_id = ?1 AND reason != ?2 AND id NOT IN (
                     SELECT id FROM node_backups WHERE node_id = ?1 AND reason != ?2
                     {NEWEST} LIMIT ?3)"
            ))?
            .execute(params![
                node.get(),
                enum_text(BackupReason::Adoption),
                i64::try_from(BACKUPS_KEPT).unwrap_or(i64::MAX),
            ])?;
            Ok(Kept::New(id))
        })
        .await
    }

    async fn backups(&self, node: NodeId) -> Result<Vec<Backup>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {BACKUP_COLUMNS} FROM node_backups WHERE node_id = ?1 {NEWEST}"
                ))?
                .query_map([node.get()], backup_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn backups_seen(&self) -> Result<BTreeMap<NodeId, Timestamp>> {
        self.read(|conn| {
            let mut seen = BTreeMap::new();
            let mut statement = conn.prepare_cached(
                "SELECT node_id, max(coalesce(seen_at, taken_at)) FROM node_backups
                 GROUP BY node_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                seen.insert(
                    NodeId::new(row.get(0)?),
                    Timestamp::from_unix_ms(row.get(1)?),
                );
            }
            Ok(seen)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_model::inventory::{BACKUPS_KEPT, BackupReason};

    use super::{BackupRepo as _, Kept};
    use crate::{
        MasterKey, NodeRepo as _, SealedBackup, Vault, repo::nodes::tests::new_node, testing,
    };

    async fn setup() -> (tempfile::TempDir, crate::Store, Vault, uniwrt_core::NodeId) {
        let (dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let node = store.create_node(new_node(2)).await.unwrap();
        (dir, store, vault, node.id)
    }

    fn at(minutes: u64) -> uniwrt_core::Timestamp {
        testing::START.saturating_add(Duration::from_secs(minutes * 60))
    }

    /// A backup taken `minutes` in, whose tarball differs each time but holds `files`.
    fn sealed(vault: &Vault, reason: BackupReason, minutes: u64, files: &str) -> SealedBackup {
        let tarball = format!("backup {minutes} of {files}");
        vault
            .seal_backup(
                reason,
                at(minutes),
                tarball.as_bytes(),
                Some(files.to_owned()),
            )
            .unwrap()
    }

    #[tokio::test]
    async fn the_same_files_only_move_seen_at_and_other_files_are_kept() {
        let (_dir, store, vault, node) = setup().await;
        let Kept::New(id) = store
            .keep_backup(node, sealed(&vault, BackupReason::Manual, 0, "a"))
            .await
            .unwrap()
        else {
            panic!("the first is new")
        };
        let kept = store
            .keep_backup(node, sealed(&vault, BackupReason::Scheduled, 10, "a"))
            .await
            .unwrap();
        let Kept::Unchanged(same) = kept else {
            panic!("the same files aren't a new backup")
        };
        assert_eq!((same.id, same.reason), (id, BackupReason::Manual));
        assert_eq!((same.taken_at, same.seen_at), (at(0), at(10)));
        assert_eq!(store.backups(node).await.unwrap(), [same]);

        let kept = store
            .keep_backup(node, sealed(&vault, BackupReason::Apply, 20, "b"))
            .await
            .unwrap();
        assert!(matches!(kept, Kept::New(_)));
        let unknown = vault
            .seal_backup(BackupReason::Manual, at(30), b"\x1f\x8b", None)
            .unwrap();
        assert!(matches!(
            store.keep_backup(node, unknown).await.unwrap(),
            Kept::New(_)
        ));
        let all = store.backups(node).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].reason, BackupReason::Apply);
        assert_eq!(store.backups_seen().await.unwrap()[&node], at(30));
    }

    #[tokio::test]
    async fn the_newest_ten_and_the_adoption_s_are_kept() {
        let (_dir, store, vault, node) = setup().await;
        let first = sealed(&vault, BackupReason::Adoption, 0, "0");
        store.keep_backup(node, first).await.unwrap();
        // The same files as the adoption's: a backup of its own, the adoption's stays as it was.
        let again = sealed(&vault, BackupReason::Manual, 1, "0");
        assert!(matches!(
            store.keep_backup(node, again).await.unwrap(),
            Kept::New(_)
        ));
        for i in 2..15u64 {
            let backup = sealed(&vault, BackupReason::Scheduled, i, &i.to_string());
            store.keep_backup(node, backup).await.unwrap();
        }
        let all = store.backups(node).await.unwrap();
        assert_eq!(all.len(), BACKUPS_KEPT + 1);
        assert_eq!(all[0].taken_at, at(14), "newest first");
        assert_eq!(all[BACKUPS_KEPT - 1].taken_at, at(5));
        assert_eq!(all[BACKUPS_KEPT].reason, BackupReason::Adoption);
    }
}

//! Adopting a node in one transaction ([`NodeRepo::adopt_node`]), and [`InventoryRepo`] on
//! SQLite: the `node_inventory` and `node_backups` tables.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    inventory::{Backup, Inventory},
    node::{HostKeyPin, NewNode},
};

use super::InventoryRepo;
use crate::{
    Error, Result, SealedBackup, Store,
    codec::{enum_text, timestamp_column},
    vault::{ALG, backup_from_row},
};

/// Everything adoption records about a node, written together: a node is never recorded
/// without the host key it must present.
pub struct Adoption {
    pub node: NewNode,
    pub pin: HostKeyPin,
    /// Its first facts, shown until the engine collects fresh ones.
    pub facts: NodeFacts,
    pub inventory: Inventory,
    pub backup: Option<SealedBackup>,
}

/// The JSON columns of an adoption, encoded off the writer thread.
pub(crate) struct Encoded {
    facts: String,
    packages: String,
}

impl Adoption {
    pub(crate) fn encode(&self) -> Result<Encoded> {
        Ok(Encoded {
            facts: serde_json::to_string(&self.facts).map_err(|source| Error::Encode {
                what: "node facts",
                source,
            })?,
            packages: serde_json::to_string(&self.inventory.packages).map_err(|source| {
                Error::Encode {
                    what: "packages",
                    source,
                }
            })?,
        })
    }
}

/// Writes the rest of an adoption for the node just inserted.
pub(crate) fn record(
    conn: &rusqlite::Connection,
    node: NodeId,
    adoption: Adoption,
    encoded: &Encoded,
) -> Result<()> {
    let Adoption {
        pin,
        facts,
        inventory,
        backup,
        ..
    } = adoption;
    conn.prepare_cached(
        "INSERT INTO node_host_keys (node_id, public_key, fingerprint, pinned_at)
         VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![
        node.get(),
        pin.public_key,
        pin.fingerprint,
        pin.pinned_at.as_unix_ms()
    ])?;
    conn.prepare_cached(
        "INSERT INTO node_facts (node_id, collected_at, facts) VALUES (?1, ?2, ?3)",
    )?
    .execute(params![
        node.get(),
        facts.collected_at.as_unix_ms(),
        encoded.facts
    ])?;
    conn.prepare_cached(
        "INSERT INTO node_inventory (node_id, taken_at, packages) VALUES (?1, ?2, ?3)",
    )?
    .execute(params![
        node.get(),
        inventory.taken_at.as_unix_ms(),
        encoded.packages
    ])?;
    if let Some(backup) = backup {
        conn.prepare_cached(
            "INSERT INTO node_backups (node_id, taken_at, reason, size, sha256, alg, value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            node.get(),
            backup.taken_at.as_unix_ms(),
            enum_text(backup.reason),
            backup.size,
            backup.sha256,
            ALG,
            backup.value,
        ])?;
    }
    Ok(())
}

impl InventoryRepo for Store {
    async fn inventory(&self, node: NodeId) -> Result<Option<Inventory>> {
        self.read(move |conn| {
            let Some((taken_at, packages)) = conn
                .prepare_cached("SELECT taken_at, packages FROM node_inventory WHERE node_id = ?1")?
                .query_row([node.get()], |row| {
                    Ok((timestamp_column(row, 0)?, row.get::<_, String>(1)?))
                })
                .optional()?
            else {
                return Ok(None);
            };
            let packages = serde_json::from_str(&packages).map_err(|source| Error::Decode {
                what: "packages",
                key: format!("node {node}"),
                source,
            })?;
            Ok(Some(Inventory { taken_at, packages }))
        })
        .await
    }

    async fn backups(&self, node: NodeId) -> Result<Vec<Backup>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT id, node_id, taken_at, reason, size, sha256 FROM node_backups
                     WHERE node_id = ?1 ORDER BY taken_at DESC, id DESC",
                )?
                .query_map([node.get()], backup_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{BackupId, MacAddr};
    use uniwrt_model::{
        facts::NodeFacts,
        inventory::{BackupReason, Inventory, Package},
        node::HostKeyPin,
    };

    use super::Adoption;
    use crate::{
        Error, FactsRepo as _, HostKeyRepo as _, InventoryRepo as _, MasterKey, NodeRepo as _,
        Vault, repo::nodes::tests::new_node, testing,
    };

    fn adoption(vault: &Vault, last_octet: u8, tarball: &[u8]) -> Adoption {
        let mut facts = NodeFacts {
            collected_at: testing::START,
            ..NodeFacts::default()
        };
        "ap".clone_into(&mut facts.board.hostname);
        Adoption {
            node: new_node(last_octet),
            pin: HostKeyPin {
                public_key: "ssh-ed25519 AAAA".to_owned(),
                fingerprint: "SHA256:abc".to_owned(),
                pinned_at: testing::START,
            },
            facts,
            inventory: Inventory {
                taken_at: testing::START,
                packages: vec![Package {
                    name: "dropbear".to_owned(),
                    version: "2024.86-r3".to_owned(),
                    explicit: true,
                }],
            },
            backup: Some(
                vault
                    .seal_backup(BackupReason::Adoption, testing::START, tarball)
                    .unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn an_adoption_records_the_node_with_its_pin_facts_inventory_and_sealed_backup() {
        let (_dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let tarball = b"\x1f\x8b not really a tarball";
        let adoption = adoption(&vault, 2, tarball);
        let (pin, inventory) = (adoption.pin.clone(), adoption.inventory.clone());
        let node = store.adopt_node(adoption).await.unwrap();

        assert_eq!(store.host_key(node.id).await.unwrap(), Some(pin));
        let facts = store.latest_facts(node.id).await.unwrap().unwrap();
        assert_eq!(facts.board.hostname, "ap");
        assert_eq!(store.inventory(node.id).await.unwrap(), Some(inventory));
        let backups = store.backups(node.id).await.unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].node, node.id);
        assert_eq!(backups[0].size_bytes, tarball.len() as u64);
        assert_eq!(backups[0].reason, BackupReason::Adoption);
        assert_eq!(backups[0].sha256.len(), 64);
        let (backup, opened) = vault.open_backup(backups[0].id).await.unwrap().unwrap();
        assert_eq!(backup, backups[0]);
        assert_eq!(opened.as_slice(), tarball);
        assert!(
            vault
                .open_backup(BackupId::new(99))
                .await
                .unwrap()
                .is_none()
        );

        // The tarball is sealed at rest.
        let stored: Vec<u8> = store
            .read(
                |conn| Ok(conn.query_row("SELECT value FROM node_backups", [], |row| row.get(0))?),
            )
            .await
            .unwrap();
        assert!(
            !stored
                .windows(tarball.len())
                .any(|window| window == tarball)
        );

        // Deleting the node deletes what adoption recorded.
        assert!(store.delete_node(node.id).await.unwrap());
        assert!(store.inventory(node.id).await.unwrap().is_none());
        assert_eq!(store.backups(node.id).await.unwrap(), []);
    }

    #[tokio::test]
    async fn adopting_a_known_mac_conflicts_and_records_nothing() {
        let (_dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let first = store.adopt_node(adoption(&vault, 2, b"one")).await.unwrap();
        let mut again = adoption(&vault, 3, b"two");
        again.node.mac = MacAddr::new([0x94, 0x83, 0xc4, 0, 0, 2]);
        let err = store.adopt_node(again).await.unwrap_err();
        assert!(
            matches!(err, Error::Conflict { what: "node", .. }),
            "{err:?}"
        );
        assert_eq!(store.nodes().await.unwrap().len(), 1);
        let backups: i64 = store
            .read(|conn| {
                Ok(conn.query_row("SELECT count(*) FROM node_backups", [], |row| row.get(0))?)
            })
            .await
            .unwrap();
        assert_eq!(backups, 1);
        assert_eq!(store.backups(first.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_backup_does_not_open_under_another_digest() {
        let (_dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let node = store.adopt_node(adoption(&vault, 2, b"one")).await.unwrap();
        let id = store.backups(node.id).await.unwrap()[0].id;
        store
            .write(|conn| {
                conn.execute("UPDATE node_backups SET sha256 = 'other'", [])?;
                Ok(())
            })
            .await
            .unwrap();
        let err = vault.open_backup(id).await.unwrap_err();
        assert!(matches!(err, Error::Unseal { .. }), "{err:?}");
    }
}

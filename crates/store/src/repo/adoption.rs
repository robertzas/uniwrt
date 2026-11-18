//! Adopting a node in one transaction ([`NodeRepo::adopt_node`]), and [`InventoryRepo`] on
//! SQLite: the `node_inventory` and `node_backups` tables.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    inventory::Inventory,
    node::{HostKeyPin, NewNode},
};

use super::InventoryRepo;
use crate::{
    Error, Result, SealedBackup, Store,
    codec::{enum_text, timestamp_column},
    vault::ALG,
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
        insert_backup(conn, node, &backup)?;
    }
    Ok(())
}

/// Writes the rest of an adoption into node `id`'s record, replacing its old device's host key,
/// facts and inventory; its backups stay.
pub(crate) fn take_over(
    conn: &rusqlite::Connection,
    node: NodeId,
    adoption: Adoption,
    encoded: &Encoded,
) -> Result<()> {
    for table in ["node_host_keys", "node_facts", "node_inventory"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE node_id = ?1"),
            [node.get()],
        )?;
    }
    record(conn, node, adoption, encoded)
}

pub(crate) fn insert_backup(
    conn: &rusqlite::Connection,
    node: NodeId,
    backup: &SealedBackup,
) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO node_backups (node_id, taken_at, reason, size, sha256, alg, value, content)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?
    .execute(params![
        node.get(),
        backup.taken_at.as_unix_ms(),
        enum_text(backup.reason),
        backup.size,
        backup.sha256,
        ALG,
        backup.value,
        backup.content,
    ])?;
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

    async fn set_inventory(&self, node: NodeId, inventory: Inventory) -> Result<()> {
        let packages =
            serde_json::to_string(&inventory.packages).map_err(|source| Error::Encode {
                what: "packages",
                source,
            })?;
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO node_inventory (node_id, taken_at, packages) VALUES (?1, ?2, ?3)
                 ON CONFLICT (node_id) DO UPDATE SET taken_at = excluded.taken_at,
                 packages = excluded.packages",
            )?
            .execute(params![
                node.get(),
                inventory.taken_at.as_unix_ms(),
                packages
            ])?;
            Ok(())
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
        BackupRepo as _, Error, FactsRepo as _, HostKeyRepo as _, InventoryRepo as _, MasterKey,
        NodeRepo as _, Vault, repo::nodes::tests::new_node, testing,
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
                    .seal_backup(BackupReason::Adoption, testing::START, tarball, None)
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
        // Another backup (before an upgrade) comes first, and opens too.
        let later = vault
            .seal_backup(
                BackupReason::Upgrade,
                testing::START.saturating_add(std::time::Duration::from_secs(60)),
                b"\x1f\x8b later",
                None,
            )
            .unwrap();
        store.keep_backup(node.id, later).await.unwrap();
        let both = store.backups(node.id).await.unwrap();
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].reason, BackupReason::Upgrade);
        let (_, opened) = vault.open_backup(both[0].id).await.unwrap().unwrap();
        assert_eq!(opened.as_slice(), b"\x1f\x8b later");
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

        // A newer read replaces the packages.
        let newer = Inventory {
            taken_at: testing::START.saturating_add(std::time::Duration::from_secs(60)),
            packages: Vec::new(),
        };
        store.set_inventory(node.id, newer.clone()).await.unwrap();
        assert_eq!(store.inventory(node.id).await.unwrap(), Some(newer));

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
    async fn a_new_device_takes_over_the_record_keeping_its_history() {
        use uniwrt_model::{node::NodeMode, ztp::ZtpApproval};

        use crate::ZtpRepo as _;

        let (_dir, store, _clock) = testing::open().await;
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let old = store.adopt_node(adoption(&vault, 2, b"old")).await.unwrap();
        let other = store
            .adopt_node(adoption(&vault, 3, b"other"))
            .await
            .unwrap();
        store
            .set_node_mode(old.id, NodeMode::Managed)
            .await
            .unwrap();
        store
            .put_ztp_approval(ZtpApproval::awaiting(&old, testing::START, None))
            .await
            .unwrap();
        assert_eq!(store.awaiting_nodes().await.unwrap(), [old.id]);

        let mut new = adoption(&vault, 9, b"new");
        "openwrt,one".clone_into(&mut new.node.board_name);
        new.node.name = "ignored".to_owned();
        new.pin.fingerprint = "SHA256:new".to_owned();
        "hall-new".clone_into(&mut new.facts.board.hostname);
        let node = store.replace_node(old.id, new).await.unwrap().unwrap();
        assert_eq!(node.id, old.id);
        assert_eq!(node.name, old.name);
        assert_eq!(node.mac, MacAddr::new([0x94, 0x83, 0xc4, 0, 0, 9]));
        assert_eq!(node.board_name, "openwrt,one");
        assert_eq!(node.host, "10.0.0.9");
        assert_eq!(node.mode, NodeMode::Observe);
        let pin = store.host_key(node.id).await.unwrap().unwrap();
        assert_eq!(pin.fingerprint, "SHA256:new");
        let facts = store.latest_facts(node.id).await.unwrap().unwrap();
        assert_eq!(facts.board.hostname, "hall-new");
        assert_eq!(store.backups(node.id).await.unwrap().len(), 2);
        assert_eq!(store.ztp_approvals(testing::START).await.unwrap(), []);
        assert_eq!(store.awaiting_nodes().await.unwrap(), []);

        // Another node's MAC conflicts, and nothing changes.
        let err = store
            .replace_node(old.id, adoption(&vault, 3, b"clash"))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Conflict { .. }), "{err:?}");
        assert_eq!(store.node(old.id).await.unwrap(), Some(node));
        assert_eq!(store.backups(other.id).await.unwrap().len(), 1);
        assert_eq!(store.backups(old.id).await.unwrap().len(), 2);
        assert!(
            store
                .replace_node(uniwrt_core::NodeId::new(99), adoption(&vault, 7, b""))
                .await
                .unwrap()
                .is_none()
        );
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

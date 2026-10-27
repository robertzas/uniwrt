//! [`HostKeyRepo`] on SQLite: the `node_host_keys` table, one pinned host key per node.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::NodeId;
use uniwrt_model::node::HostKeyPin;

use super::HostKeyRepo;
use crate::{Result, Store, codec::constraint, codec::timestamp_column};

impl HostKeyRepo for Store {
    async fn pin_host_key(&self, node: NodeId, pin: HostKeyPin) -> Result<()> {
        self.write(move |conn| {
            let HostKeyPin {
                public_key,
                fingerprint,
                pinned_at,
            } = pin;
            conn.prepare_cached(
                "INSERT INTO node_host_keys (node_id, public_key, fingerprint, pinned_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (node_id) DO UPDATE
                 SET public_key = excluded.public_key, fingerprint = excluded.fingerprint,
                     pinned_at = excluded.pinned_at",
            )?
            .execute(params![
                node.get(),
                public_key,
                fingerprint,
                pinned_at.as_unix_ms()
            ])
            .map_err(|err| constraint(err, "node", node))?;
            Ok(())
        })
        .await
    }

    async fn host_key(&self, node: NodeId) -> Result<Option<HostKeyPin>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT public_key, fingerprint, pinned_at FROM node_host_keys
                     WHERE node_id = ?1",
                )?
                .query_row([node.get()], |row| {
                    Ok(HostKeyPin {
                        public_key: row.get(0)?,
                        fingerprint: row.get(1)?,
                        pinned_at: timestamp_column(row, 2)?,
                    })
                })
                .optional()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{NodeId, Timestamp};
    use uniwrt_model::node::HostKeyPin;

    use crate::{Error, HostKeyRepo as _, NodeRepo as _, repo::nodes::tests::new_node, testing};

    fn pin(key: &str, at: i64) -> HostKeyPin {
        HostKeyPin {
            public_key: format!("ssh-ed25519 {key}"),
            fingerprint: format!("SHA256:{key}"),
            pinned_at: Timestamp::from_unix_ms(at),
        }
    }

    #[tokio::test]
    async fn a_pin_is_stored_replaced_on_retrust_and_dropped_with_its_node() {
        let (_dir, store, _) = testing::open().await;
        let node = store.create_node(new_node(1)).await.unwrap().id;
        assert_eq!(store.host_key(node).await.unwrap(), None);

        store.pin_host_key(node, pin("first", 1)).await.unwrap();
        assert_eq!(store.host_key(node).await.unwrap(), Some(pin("first", 1)));
        store.pin_host_key(node, pin("reflashed", 2)).await.unwrap();
        assert_eq!(
            store.host_key(node).await.unwrap(),
            Some(pin("reflashed", 2))
        );

        store.delete_node(node).await.unwrap();
        assert_eq!(store.host_key(node).await.unwrap(), None);
    }

    #[tokio::test]
    async fn pinning_for_an_unknown_node_is_refused() {
        let (_dir, store, _) = testing::open().await;
        let err = store
            .pin_host_key(NodeId::new(9), pin("x", 1))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, Error::NotFound { what: "node", key } if key == "9"),
            "{err:?}"
        );
    }
}

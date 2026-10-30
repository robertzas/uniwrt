//! [`NodeRepo`] on SQLite: the `nodes` table.

use rusqlite::{Connection, OptionalExtension as _, Row, params};
use uniwrt_core::NodeId;
use uniwrt_model::node::{NewNode, Node, NodeMode};

use super::{NodeRepo, adoption};
use crate::{
    Adoption, Result, Store,
    codec::{constraint, enum_column, enum_text, mac_column, timestamp_column},
};

fn node_from_row(row: &Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: NodeId::new(row.get(0)?),
        name: row.get(1)?,
        mac: mac_column(row, 2)?,
        board_name: row.get(3)?,
        host: row.get(4)?,
        port: row.get(5)?,
        role: enum_column(row, 6)?,
        mode: enum_column(row, 7)?,
        created_at: timestamp_column(row, 8)?,
        updated_at: timestamp_column(row, 9)?,
    })
}

fn insert(conn: &Connection, node: NewNode, now: i64) -> Result<Node> {
    let NewNode {
        name,
        mac,
        board_name,
        host,
        port,
        role,
    } = node;
    conn.prepare_cached(
        "INSERT INTO nodes (name, mac, board_name, host, port, role, mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         RETURNING id, name, mac, board_name, host, port, role, mode, created_at, updated_at",
    )?
    .query_row(
        params![
            name,
            mac.to_string(),
            board_name,
            host,
            port,
            enum_text(role),
            enum_text(NodeMode::Observe),
            now,
        ],
        node_from_row,
    )
    .map_err(|err| constraint(err, "node", mac))
}

impl NodeRepo for Store {
    async fn create_node(&self, node: NewNode) -> Result<Node> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| insert(conn, node, now)).await
    }

    async fn adopt_node(&self, adoption: Adoption) -> Result<Node> {
        let now = self.now().as_unix_ms();
        let encoded = adoption.encode()?;
        self.write(move |conn| {
            let node = insert(conn, adoption.node.clone(), now)?;
            adoption::record(conn, node.id, adoption, &encoded)?;
            Ok(node)
        })
        .await
    }

    async fn node(&self, id: NodeId) -> Result<Option<Node>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT id, name, mac, board_name, host, port, role, mode, created_at, updated_at
                     FROM nodes WHERE id = ?1",
                )?
                .query_row([id.get()], node_from_row)
                .optional()?)
        })
        .await
    }

    async fn nodes(&self) -> Result<Vec<Node>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT id, name, mac, board_name, host, port, role, mode, created_at, updated_at
                     FROM nodes ORDER BY id",
                )?
                .query_map([], node_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn delete_node(&self, id: NodeId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM nodes WHERE id = ?1")?
                .execute([id.get()])?
                > 0)
        })
        .await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use uniwrt_core::{MacAddr, NodeId};
    use uniwrt_model::node::{NewNode, NodeMode, NodeRole};

    use crate::{Error, NodeRepo as _, testing};

    pub(crate) fn new_node(last_octet: u8) -> NewNode {
        NewNode {
            name: format!("ap{last_octet}"),
            mac: MacAddr::new([0x94, 0x83, 0xc4, 0, 0, last_octet]),
            board_name: "glinet,gl-mt6000".to_owned(),
            host: format!("10.0.0.{last_octet}"),
            port: 22,
            role: NodeRole::Ap,
        }
    }

    #[tokio::test]
    async fn created_nodes_start_in_observe_mode_and_read_back() {
        let (_dir, store, clock) = testing::open().await;
        let first = store.create_node(new_node(2)).await.unwrap();
        clock.advance(std::time::Duration::from_secs(1));
        let second = store.create_node(new_node(3)).await.unwrap();

        assert_eq!(first.mode, NodeMode::Observe);
        assert_eq!(
            (first.created_at, first.updated_at),
            (testing::START, testing::START)
        );
        assert_eq!(first.mac, new_node(2).mac);
        assert!(second.id > first.id);
        assert_eq!(store.node(first.id).await.unwrap(), Some(first.clone()));
        assert_eq!(store.nodes().await.unwrap(), [first, second]);
        assert_eq!(store.node(NodeId::new(99)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_second_node_with_the_same_mac_conflicts() {
        let (_dir, store, _) = testing::open().await;
        store.create_node(new_node(2)).await.unwrap();
        let mut twin = new_node(2);
        twin.host = "10.0.0.99".to_owned();
        let err = store.create_node(twin).await.unwrap_err();
        assert!(
            matches!(&err, Error::Conflict { what: "node", key } if key == "94:83:c4:00:00:02"),
            "{err:?}"
        );
        assert_eq!(store.nodes().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleted_ids_are_never_reused() {
        let (_dir, store, _) = testing::open().await;
        let first = store.create_node(new_node(2)).await.unwrap();
        assert!(store.delete_node(first.id).await.unwrap());
        assert!(!store.delete_node(first.id).await.unwrap());
        let next = store.create_node(new_node(2)).await.unwrap();
        assert!(next.id > first.id);
    }

    #[tokio::test]
    async fn nodes_survive_reopening() {
        let (dir, store, clock) = testing::open().await;
        let node = store.create_node(new_node(2)).await.unwrap();
        store.close().await;
        let reopened = crate::Store::open(dir.path().join("test.db"), std::sync::Arc::new(clock))
            .await
            .unwrap();
        assert_eq!(reopened.nodes().await.unwrap(), [node]);
    }

    #[tokio::test]
    async fn an_unknown_stored_role_is_a_decode_error() {
        let (_dir, store, _) = testing::open().await;
        let node = store.create_node(new_node(2)).await.unwrap();
        store
            .write(move |conn| {
                conn.execute(
                    "UPDATE nodes SET role = 'router' WHERE id = ?1",
                    [node.id.get()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let err = store.nodes().await.unwrap_err();
        assert!(
            matches!(
                err,
                Error::Sqlite(rusqlite::Error::FromSqlConversionFailure(6, ..))
            ),
            "{err:?}"
        );
    }
}

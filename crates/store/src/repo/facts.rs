//! [`FactsRepo`] on SQLite: the `node_facts` table, one JSON snapshot per node.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::NodeId;
use uniwrt_model::facts::NodeFacts;

use super::FactsRepo;
use crate::{Error, Result, Store, codec::constraint};

impl FactsRepo for Store {
    async fn save_facts(&self, node: NodeId, facts: &NodeFacts) -> Result<()> {
        // Encoded here rather than on the single writer thread, which only runs SQL.
        let json = serde_json::to_string(facts).map_err(|source| Error::Encode {
            what: "node facts",
            source,
        })?;
        let collected_at = facts.collected_at.as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO node_facts (node_id, collected_at, facts) VALUES (?1, ?2, ?3)
                 ON CONFLICT (node_id) DO UPDATE
                 SET collected_at = excluded.collected_at, facts = excluded.facts
                 WHERE excluded.collected_at >= node_facts.collected_at",
            )?
            .execute(params![node.get(), collected_at, json])
            .map_err(|err| constraint(err, "node", node))?;
            Ok(())
        })
        .await
    }

    async fn latest_facts(&self, node: NodeId) -> Result<Option<NodeFacts>> {
        self.read(move |conn| {
            let json: Option<String> = conn
                .prepare_cached("SELECT facts FROM node_facts WHERE node_id = ?1")?
                .query_row([node.get()], |row| row.get(0))
                .optional()?;
            // Decoded on the blocking pool, off the async workers.
            json.map(|json| {
                serde_json::from_str(&json).map_err(|source| Error::Decode {
                    what: "facts",
                    key: format!("node {node}"),
                    source,
                })
            })
            .transpose()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{NodeId, Timestamp};
    use uniwrt_model::facts::NodeFacts;

    use crate::{Error, FactsRepo as _, NodeRepo as _, repo::nodes::tests::new_node, testing};

    const GATEWAY: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../model/fixtures/facts/gateway.json"
    ));

    fn facts_at(ms: i64, hostname: &str) -> NodeFacts {
        let mut facts = NodeFacts {
            collected_at: Timestamp::from_unix_ms(ms),
            ..NodeFacts::default()
        };
        hostname.clone_into(&mut facts.board.hostname);
        facts
    }

    #[tokio::test]
    async fn the_latest_snapshot_round_trips() {
        let (_dir, store, _) = testing::open().await;
        let node = store.create_node(new_node(1)).await.unwrap().id;
        assert_eq!(store.latest_facts(node).await.unwrap(), None);
        let facts: NodeFacts = serde_json::from_str(GATEWAY).unwrap();
        store.save_facts(node, &facts).await.unwrap();
        assert_eq!(store.latest_facts(node).await.unwrap(), Some(facts));
    }

    #[tokio::test]
    async fn an_older_snapshot_never_replaces_a_newer_one() {
        let (_dir, store, _) = testing::open().await;
        let node = store.create_node(new_node(1)).await.unwrap().id;
        store
            .save_facts(node, &facts_at(2_000, "new"))
            .await
            .unwrap();
        store
            .save_facts(node, &facts_at(1_000, "old"))
            .await
            .unwrap();
        assert_eq!(
            store.latest_facts(node).await.unwrap(),
            Some(facts_at(2_000, "new"))
        );
        store
            .save_facts(node, &facts_at(3_000, "newer"))
            .await
            .unwrap();
        assert_eq!(
            store.latest_facts(node).await.unwrap(),
            Some(facts_at(3_000, "newer"))
        );
    }

    #[tokio::test]
    async fn facts_of_an_unknown_node_are_refused_and_deleting_a_node_drops_its_facts() {
        let (_dir, store, _) = testing::open().await;
        let err = store
            .save_facts(NodeId::new(9), &facts_at(1, "x"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, Error::NotFound { what: "node", key } if key == "9"),
            "{err:?}"
        );

        let node = store.create_node(new_node(1)).await.unwrap().id;
        store.save_facts(node, &facts_at(1, "x")).await.unwrap();
        store.delete_node(node).await.unwrap();
        let rows: i64 = store
            .read(|conn| {
                Ok(conn.query_row("SELECT count(*) FROM node_facts", [], |row| row.get(0))?)
            })
            .await
            .unwrap();
        assert_eq!(rows, 0);
    }
}

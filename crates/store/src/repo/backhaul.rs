//! [`BackhaulRepo`] on SQLite: the `backhaul_failures` table.

use rusqlite::params;
use uniwrt_core::{JobId, NodeId};
use uniwrt_model::apply::{BackhaulFailure, CableEnd};

use super::BackhaulRepo;
use crate::{Result, Store, codec::timestamp_column};

/// The cable's ends in order, the row's key.
fn ordered(ends: [CableEnd; 2]) -> [CableEnd; 2] {
    let [a, b] = ends;
    if a <= b { [a, b] } else { [b, a] }
}

impl BackhaulRepo for Store {
    async fn record_backhaul_failure(&self, failure: BackhaulFailure) -> Result<()> {
        let [a, b] = ordered(failure.ends);
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO backhaul_failures (a_node, a_port, b_node, b_port, job_id, at, why)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (a_node, a_port, b_node, b_port)
                 DO UPDATE SET job_id = excluded.job_id, at = excluded.at, why = excluded.why",
            )?
            .execute(params![
                a.node.get(),
                a.port,
                b.node.get(),
                b.port,
                failure.job.get(),
                failure.at.as_unix_ms(),
                failure.why,
            ])?;
            Ok(())
        })
        .await
    }

    async fn backhaul_failures(&self) -> Result<Vec<BackhaulFailure>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT a_node, a_port, b_node, b_port, job_id, at, why
                     FROM backhaul_failures ORDER BY a_node, a_port, b_node, b_port",
                )?
                .query_map([], |row| {
                    Ok(BackhaulFailure {
                        ends: [
                            CableEnd {
                                node: NodeId::new(row.get(0)?),
                                port: row.get(1)?,
                            },
                            CableEnd {
                                node: NodeId::new(row.get(2)?),
                                port: row.get(3)?,
                            },
                        ],
                        job: JobId::new(row.get(4)?),
                        at: timestamp_column(row, 5)?,
                        why: row.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn forget_backhaul_failure(&self, ends: [CableEnd; 2]) -> Result<bool> {
        let [a, b] = ordered(ends);
        self.write(move |conn| {
            let removed = conn
                .prepare_cached(
                    "DELETE FROM backhaul_failures
                     WHERE a_node = ?1 AND a_port = ?2 AND b_node = ?3 AND b_port = ?4",
                )?
                .execute(params![a.node.get(), a.port, b.node.get(), b.port])?;
            Ok(removed > 0)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::JobId;
    use uniwrt_model::apply::{BackhaulFailure, CableEnd};

    use crate::{BackhaulRepo as _, NodeRepo as _, repo::nodes::tests::new_node, testing};

    #[tokio::test]
    async fn a_failed_cable_is_kept_once_whichever_end_names_it_first() {
        let (_dir, store, _) = testing::open().await;
        let a = store.create_node(new_node(1)).await.unwrap().id;
        let b = store.create_node(new_node(2)).await.unwrap().id;
        let end = |node, port: &str| CableEnd {
            node,
            port: port.to_owned(),
        };
        let failure = |ends, job, why: &str| BackhaulFailure {
            ends,
            job: JobId::new(job),
            at: testing::START.saturating_add(Duration::from_secs(job.unsigned_abs())),
            why: why.to_owned(),
        };
        store
            .record_backhaul_failure(failure([end(b, "lan2"), end(a, "lan1")], 7, "1500"))
            .await
            .unwrap();
        // The same cable again, named from the other end: the newer probe stays.
        let newer = failure([end(a, "lan1"), end(b, "lan2")], 9, "no answer");
        store.record_backhaul_failure(newer.clone()).await.unwrap();
        assert_eq!(store.backhaul_failures().await.unwrap(), [newer]);
        assert!(
            store
                .forget_backhaul_failure([end(b, "lan2"), end(a, "lan1")])
                .await
                .unwrap()
        );
        assert_eq!(store.backhaul_failures().await.unwrap(), []);
        assert!(
            !store
                .forget_backhaul_failure([end(a, "lan1"), end(b, "lan2")])
                .await
                .unwrap()
        );
    }
}

//! [`ApplyRepo`] on SQLite: the `node_applied` table, and the nodes' addresses an apply moved.

use rusqlite::params;
use uniwrt_core::{JobId, NodeId, RevisionId};
use uniwrt_model::apply::{Applied, AppliedNode, AppliedRevision};

use super::{ApplyRepo, drift::sha256};
use crate::{Result, Store, codec::timestamp_column};

impl ApplyRepo for Store {
    async fn record_applied(&self, applied: Applied) -> Result<()> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            // A write runs in its own savepoint: all or nothing.
            for AppliedNode {
                node,
                holds,
                host,
                packages,
            } in &applied.nodes
            {
                if let Some(host) = host {
                    conn.prepare_cached(
                        "UPDATE nodes SET host = ?2, updated_at = ?3 WHERE id = ?1",
                    )?
                    .execute(params![node.get(), host, now])?;
                }
                if *holds {
                    conn.prepare_cached(
                        "INSERT INTO node_applied (node_id, revision_id, job_id, applied_at)
                         SELECT id, ?2, ?3, ?4 FROM nodes WHERE id = ?1
                         ON CONFLICT (node_id) DO UPDATE SET revision_id = excluded.revision_id,
                         job_id = excluded.job_id, applied_at = excluded.applied_at",
                    )?
                    .execute(params![
                        node.get(),
                        applied.revision.get(),
                        applied.job.get(),
                        now
                    ])?;
                }
                conn.prepare_cached("DELETE FROM node_config WHERE node_id = ?1")?
                    .execute(params![node.get()])?;
                for (package, text) in packages {
                    conn.prepare_cached(
                        "INSERT INTO node_config (node_id, package, sha256, text, revision_id,
                         confirmed_at)
                         SELECT id, ?2, ?3, ?4, ?5, ?6 FROM nodes WHERE id = ?1",
                    )?
                    .execute(params![
                        node.get(),
                        package,
                        sha256(text),
                        text,
                        applied.revision.get(),
                        now
                    ])?;
                }
            }
            Ok(())
        })
        .await
    }

    async fn applied_revisions(&self) -> Result<Vec<AppliedRevision>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT node_id, revision_id, job_id, applied_at FROM node_applied
                     ORDER BY node_id",
                )?
                .query_map([], |row| {
                    Ok(AppliedRevision {
                        node: NodeId::new(row.get(0)?),
                        revision: RevisionId::new(row.get(1)?),
                        job: JobId::new(row.get(2)?),
                        at: timestamp_column(row, 3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::{Clock as _, JobId, MacAddr, NodeId};
    use uniwrt_model::{
        apply::{Applied, AppliedNode},
        node::{NewNode, NodeRole},
        site::{
            SiteConfig,
            revision::{NewRevision, RevisionKind},
        },
    };

    use crate::{ApplyRepo as _, DriftRepo as _, NodeRepo as _, RevisionRepo as _, testing};

    #[tokio::test]
    async fn applies_record_the_revision_and_move_addresses() {
        let (_dir, store, clock) = testing::open().await;
        let node = store
            .create_node(NewNode {
                name: "ap".to_owned(),
                mac: MacAddr::new([2, 0, 0, 0, 0, 1]),
                board_name: "x86".to_owned(),
                host: "192.168.1.20".to_owned(),
                port: 22,
                role: NodeRole::Ap,
            })
            .await
            .unwrap();
        let revision = store
            .commit_revision(
                NewRevision {
                    base: None,
                    kind: RevisionKind::Defaults,
                    restored_from: None,
                    author_id: None,
                    author: None,
                    message: "defaults".to_owned(),
                    config: SiteConfig::defaults(Some("fd00:1:2::/48".parse().unwrap())),
                },
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(store.applied_revisions().await.unwrap(), []);
        clock.advance(Duration::from_secs(5));
        store
            .record_applied(Applied {
                revision: revision.id,
                job: JobId::new(3),
                nodes: vec![
                    applied_node(node.id, Some("10.0.0.20"), &[("network", "config x\n")]),
                    applied_node(NodeId::new(99), None, &[]),
                ],
            })
            .await
            .unwrap();
        let applied = store.applied_revisions().await.unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!(
            (
                applied[0].node,
                applied[0].revision,
                applied[0].job,
                applied[0].at
            ),
            (node.id, revision.id, JobId::new(3), clock.now())
        );
        let moved = store.node(node.id).await.unwrap().unwrap();
        assert_eq!(moved.host, "10.0.0.20");
        store
            .record_applied(Applied {
                revision: revision.id,
                job: JobId::new(4),
                nodes: vec![applied_node(node.id, None, &[("dhcp", "config y\n")])],
            })
            .await
            .unwrap();
        assert_eq!(
            store.applied_revisions().await.unwrap()[0].job,
            JobId::new(4)
        );
        assert_eq!(
            store.node(node.id).await.unwrap().unwrap().host,
            "10.0.0.20"
        );
        // The second apply replaced the node's confirmed packages.
        let confirmed = store.confirmed(Some(node.id)).await.unwrap();
        assert_eq!(confirmed.len(), 1);
        assert_eq!(
            (confirmed[0].package.as_str(), confirmed[0].text.as_str()),
            ("dhcp", "config y\n")
        );
        assert_eq!(confirmed[0].sha256, super::sha256("config y\n"));
        assert_eq!(confirmed[0].revision, revision.id);
    }

    fn applied_node(node: NodeId, host: Option<&str>, packages: &[(&str, &str)]) -> AppliedNode {
        AppliedNode {
            node,
            holds: true,
            host: host.map(str::to_owned),
            packages: packages
                .iter()
                .map(|(p, t)| ((*p).to_owned(), (*t).to_owned()))
                .collect(),
        }
    }
}

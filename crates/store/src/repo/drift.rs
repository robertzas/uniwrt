//! [`DriftRepo`] on SQLite: the `node_config` table (what the controller confirmed, written by
//! [`ApplyRepo::record_applied`](super::ApplyRepo::record_applied)), `config_drift` and
//! `drift_mutes`.

use std::fmt::Write as _;

use rusqlite::{OptionalExtension as _, Row, params};
use uniwrt_core::{NodeId, RevisionId, Timestamp};
use uniwrt_model::drift::{Acknowledged, Confirmed, DriftMute, DriftRecord};

use super::DriftRepo;
use crate::{Result, Store, codec::timestamp_column};

/// SHA-256 of `text`, lowercase hex, as `sha256sum` prints it.
pub(crate) fn sha256(text: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, text.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn confirmed(row: &Row<'_>) -> rusqlite::Result<Confirmed> {
    Ok(Confirmed {
        node: NodeId::new(row.get(0)?),
        package: row.get(1)?,
        sha256: row.get(2)?,
        text: row.get(3)?,
        revision: RevisionId::new(row.get(4)?),
        at: timestamp_column(row, 5)?,
    })
}

fn drift(row: &Row<'_>) -> rusqlite::Result<DriftRecord> {
    let acknowledged_at: Option<i64> = row.get(6)?;
    Ok(DriftRecord {
        node: NodeId::new(row.get(0)?),
        package: row.get(1)?,
        sha256: row.get(2)?,
        text: row.get(3)?,
        detected_at: timestamp_column(row, 4)?,
        changed_at: timestamp_column(row, 5)?,
        acknowledged: acknowledged_at.map(|at| Acknowledged {
            at: Timestamp::from_unix_ms(at),
            by: row.get(7).ok().flatten(),
        }),
    })
}

impl DriftRepo for Store {
    async fn confirmed(&self, node: Option<NodeId>) -> Result<Vec<Confirmed>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT node_id, package, sha256, text, revision_id, confirmed_at
                     FROM node_config WHERE ?1 IS NULL OR node_id = ?1
                     ORDER BY node_id, package",
                )?
                .query_map(params![node.map(NodeId::get)], confirmed)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn reconfirm(
        &self,
        node: NodeId,
        package: String,
        sha256: String,
        text: String,
    ) -> Result<()> {
        self.write(move |conn| {
            conn.prepare_cached(
                "UPDATE node_config SET sha256 = ?3, text = ?4 WHERE node_id = ?1 AND package = ?2",
            )?
            .execute(params![node.get(), package, sha256, text])?;
            Ok(())
        })
        .await
    }

    async fn drifts(&self) -> Result<Vec<DriftRecord>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT node_id, package, sha256, text, detected_at, changed_at,
                     acknowledged_at, acknowledged_by FROM config_drift ORDER BY node_id, package",
                )?
                .query_map([], drift)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn put_drift(&self, record: DriftRecord) -> Result<()> {
        self.write(move |conn| {
            let (at, by) = record
                .acknowledged
                .map_or((None, None), |ack| (Some(ack.at.as_unix_ms()), ack.by));
            conn.prepare_cached(
                "INSERT INTO config_drift (node_id, package, sha256, text, detected_at, changed_at,
                 acknowledged_at, acknowledged_by)
                 SELECT id, ?2, ?3, ?4, ?5, ?6, ?7, ?8 FROM nodes WHERE id = ?1
                 ON CONFLICT (node_id, package) DO UPDATE SET sha256 = excluded.sha256,
                 text = excluded.text, detected_at = excluded.detected_at,
                 changed_at = excluded.changed_at, acknowledged_at = excluded.acknowledged_at,
                 acknowledged_by = excluded.acknowledged_by",
            )?
            .execute(params![
                record.node.get(),
                record.package,
                record.sha256,
                record.text,
                record.detected_at.as_unix_ms(),
                record.changed_at.as_unix_ms(),
                at,
                by
            ])?;
            Ok(())
        })
        .await
    }

    async fn delete_drift(&self, node: NodeId, package: String) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM config_drift WHERE node_id = ?1 AND package = ?2")?
                .execute(params![node.get(), package])?
                > 0)
        })
        .await
    }

    async fn drift_mutes(&self) -> Result<Vec<DriftMute>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT node_id, package, muted_at, muted_by FROM drift_mutes
                     ORDER BY node_id, package",
                )?
                .query_map([], |row| {
                    Ok(DriftMute {
                        node: NodeId::new(row.get(0)?),
                        package: row.get(1)?,
                        at: timestamp_column(row, 2)?,
                        by: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn set_drift_mute(
        &self,
        node: NodeId,
        package: String,
        muted: bool,
        by: Option<String>,
    ) -> Result<bool> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let changed = if muted {
                let known: Option<i64> = conn
                    .prepare_cached(
                        "SELECT 1 FROM drift_mutes WHERE node_id = ?1 AND package = ?2",
                    )?
                    .query_row(params![node.get(), package], |row| row.get(0))
                    .optional()?;
                known.is_none()
                    && conn
                        .prepare_cached(
                            "INSERT INTO drift_mutes (node_id, package, muted_at, muted_by)
                                 SELECT id, ?2, ?3, ?4 FROM nodes WHERE id = ?1",
                        )?
                        .execute(params![node.get(), package, now, by])?
                        > 0
            } else {
                conn.prepare_cached("DELETE FROM drift_mutes WHERE node_id = ?1 AND package = ?2")?
                    .execute(params![node.get(), package])?
                    > 0
            };
            Ok(changed)
        })
        .await
    }
}

#[cfg(test)]
#[path = "drift_tests.rs"]
mod tests;

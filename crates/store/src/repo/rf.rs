//! [`RfRepo`] on SQLite: the `rf_scans` table, what each scan heard as JSON.

use rusqlite::{OptionalExtension as _, Row, params};
use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, ScanId, Timestamp};
use uniwrt_model::rf::{Bss, ChannelSurvey, Heard, NewScan, Scan, ScanSummary};

use super::RfRepo;
use crate::{
    Error, Result, Store,
    codec::{enum_column, enum_text, timestamp_column},
};

const SUMMARY: &str = "id, node_id, radio, band, at, trigger, duration_ms, networks";

/// The `heard` column.
#[derive(Serialize, Deserialize)]
struct Stored {
    bsss: Vec<Bss>,
    channels: Vec<ChannelSurvey>,
}

impl RfRepo for Store {
    async fn add_scan(&self, scan: NewScan) -> Result<Scan> {
        let Heard {
            duration_ms,
            bsss,
            channels,
        } = scan.heard;
        let stored = Stored { bsss, channels };
        let heard = serde_json::to_string(&stored).map_err(|source| Error::Encode {
            what: "scan",
            source,
        })?;
        let networks = i64::try_from(stored.bsss.len()).unwrap_or(i64::MAX);
        let row = (
            scan.node.get(),
            scan.radio.clone(),
            enum_text(scan.band),
            scan.at.as_unix_ms(),
            enum_text(scan.trigger),
            duration_ms,
        );
        let id = self
            .write(move |conn| {
                Ok(conn
                    .prepare_cached(
                        "INSERT INTO rf_scans
                             (node_id, radio, band, at, trigger, duration_ms, networks, heard)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
                    )?
                    .query_row(
                        params![row.0, row.1, row.2, row.3, row.4, row.5, networks, heard],
                        |r| r.get(0),
                    )?)
            })
            .await?;
        let scan = NewScan {
            heard: Heard {
                duration_ms,
                bsss: stored.bsss,
                channels: stored.channels,
            },
            ..scan
        };
        Ok(Scan::new(ScanId::new(id), scan))
    }

    async fn latest_scans(&self, since: Timestamp) -> Result<Vec<Scan>> {
        let since = since.as_unix_ms();
        self.read(move |conn| {
            let query = format!(
                "SELECT {SUMMARY}, heard FROM rf_scans AS s
                 WHERE at >= ?1 AND id = (
                     SELECT id FROM rf_scans
                     WHERE node_id = s.node_id AND radio = s.radio
                     ORDER BY at DESC, id DESC LIMIT 1)
                 ORDER BY node_id, radio"
            );
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([since], full)?;
            rows.map(|row| decode(row?)).collect()
        })
        .await
    }

    async fn scans(
        &self,
        node: Option<NodeId>,
        radio: Option<String>,
        before: Option<ScanId>,
        limit: usize,
    ) -> Result<Vec<ScanSummary>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            let query = format!(
                "SELECT {SUMMARY} FROM rf_scans
                 WHERE (?1 IS NULL OR node_id = ?1) AND (?2 IS NULL OR radio = ?2)
                     AND (?3 IS NULL OR id < ?3)
                 ORDER BY id DESC LIMIT ?4"
            );
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map(
                params![node.map(NodeId::get), radio, before.map(ScanId::get), limit],
                summary,
            )?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn scan(&self, id: ScanId) -> Result<Option<Scan>> {
        self.read(move |conn| {
            let query = format!("SELECT {SUMMARY}, heard FROM rf_scans WHERE id = ?1");
            conn.prepare_cached(&query)?
                .query_row([id.get()], full)
                .optional()?
                .map(decode)
                .transpose()
        })
        .await
    }

    async fn prune_scans(&self, before: Timestamp) -> Result<usize> {
        let before = before.as_unix_ms();
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM rf_scans WHERE at < ?1")?
                .execute([before])?)
        })
        .await
    }
}

fn summary(row: &Row<'_>) -> rusqlite::Result<ScanSummary> {
    Ok(ScanSummary {
        id: ScanId::new(row.get(0)?),
        node: NodeId::new(row.get(1)?),
        radio: row.get(2)?,
        band: enum_column(row, 3)?,
        at: timestamp_column(row, 4)?,
        trigger: enum_column(row, 5)?,
        duration_ms: row.get(6)?,
        networks: row.get(7)?,
    })
}

fn full(row: &Row<'_>) -> rusqlite::Result<(ScanSummary, String)> {
    Ok((summary(row)?, row.get(8)?))
}

fn decode((summary, heard): (ScanSummary, String)) -> Result<Scan> {
    let stored: Stored = serde_json::from_str(&heard).map_err(|source| Error::Decode {
        what: "scan",
        key: summary.id.to_string(),
        source,
    })?;
    Ok(Scan {
        id: summary.id,
        node: summary.node,
        radio: summary.radio,
        band: summary.band,
        at: summary.at,
        trigger: summary.trigger,
        duration_ms: summary.duration_ms,
        bsss: stored.bsss,
        channels: stored.channels,
    })
}

#[cfg(test)]
#[path = "rf_tests.rs"]
mod tests;

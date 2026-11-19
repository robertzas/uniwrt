//! [`DnsQueryRepo`] on SQLite: the `dns_queries` and `dns_hours` tables.

use rusqlite::params;
use uniwrt_core::Timestamp;
use uniwrt_model::dns_queries::{DnsHour, DnsQuery};

use super::{DnsPrune, DnsQueryRepo};
use crate::{
    Result, Store,
    codec::{enum_column, enum_text, invalid, timestamp_column},
};

/// The most rows one prune deletes, so a long catch-up doesn't hold the writer.
const PRUNE_BATCH: i64 = 20_000;

/// The start of the hour holding `at`.
fn hour_of(at: Timestamp) -> i64 {
    at.as_unix_ms().div_euclid(3_600_000) * 3_600_000
}

fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn count_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|err| invalid(index, err))
}

impl DnsQueryRepo for Store {
    async fn record_dns(&self, queries: Vec<DnsQuery>, hours: Vec<DnsHour>) -> Result<()> {
        if queries.is_empty() && hours.is_empty() {
            return Ok(());
        }
        self.write(move |conn| {
            let mut insert = conn.prepare_cached(
                "INSERT INTO dns_queries (at, client, mac, qtype, domain, outcome)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for query in &queries {
                insert.execute(params![
                    query.at.as_unix_ms(),
                    query.client.to_string(),
                    query.mac.map(|mac| mac.to_string()),
                    query.qtype,
                    query.domain,
                    enum_text(query.outcome),
                ])?;
            }
            let mut add = conn.prepare_cached(
                "INSERT INTO dns_hours (hour, client, queries, blocked) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (hour, client) DO UPDATE SET
                     queries = queries + excluded.queries, blocked = blocked + excluded.blocked",
            )?;
            for hour in &hours {
                add.execute(params![
                    hour_of(hour.hour),
                    hour.client,
                    count(hour.queries),
                    count(hour.blocked),
                ])?;
            }
            Ok(())
        })
        .await
    }

    async fn dns_queries(&self, since: Timestamp, limit: usize) -> Result<Vec<DnsQuery>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT at, client, mac, qtype, domain, outcome FROM dns_queries
                     WHERE at >= ?1 ORDER BY at DESC, id DESC LIMIT ?2",
                )?
                .query_map(params![since.as_unix_ms(), limit], |row| {
                    let client: String = row.get(1)?;
                    let mac: Option<String> = row.get(2)?;
                    Ok(DnsQuery {
                        at: timestamp_column(row, 0)?,
                        client: client.parse().map_err(|err| invalid(1, err))?,
                        mac: mac
                            .map(|mac| mac.parse())
                            .transpose()
                            .map_err(|err| invalid(2, err))?,
                        qtype: row.get(3)?,
                        domain: row.get(4)?,
                        outcome: enum_column(row, 5)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn dns_hours(&self, since: Timestamp) -> Result<Vec<DnsHour>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT hour, client, queries, blocked FROM dns_hours
                     WHERE hour >= ?1 ORDER BY hour, client",
                )?
                .query_map([hour_of(since)], |row| {
                    Ok(DnsHour {
                        hour: timestamp_column(row, 0)?,
                        client: row.get(1)?,
                        queries: count_column(row, 2)?,
                        blocked: count_column(row, 3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn prune_dns(&self, prune: DnsPrune) -> Result<usize> {
        self.write(move |conn| {
            let queries = conn.execute(
                "DELETE FROM dns_queries WHERE id IN
                     (SELECT id FROM dns_queries WHERE at < ?1 ORDER BY at LIMIT ?2)",
                params![prune.queries_before.as_unix_ms(), PRUNE_BATCH],
            )?;
            let hours = conn.execute(
                "DELETE FROM dns_hours WHERE hour < ?1",
                [hour_of(prune.hours_before)],
            )?;
            Ok(queries + hours)
        })
        .await
    }
}

//! [`DnsQueryRepo`] on SQLite: the `dns_queries` and `dns_hours` tables, and the statistics'
//! reads over them (pages of queries, totals, top domains).

use rusqlite::{params, params_from_iter, types::Value};
use uniwrt_core::Timestamp;
use uniwrt_model::dns_queries::{
    DnsClientTotal, DnsHour, DnsHourTotal, DnsQuery, DomainCount, Outcome,
};

use super::{DnsClient, DnsPrune, DnsQueryFilter, DnsQueryRepo, DnsTotals};
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

/// `text` with `LIKE`'s wildcards and its escape character escaped by `\`.
fn like_escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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

    async fn dns_queries(
        &self,
        filter: DnsQueryFilter,
        before: Option<(Timestamp, i64)>,
        limit: usize,
    ) -> Result<Vec<(i64, DnsQuery)>> {
        self.read(move |conn| {
            let (mut clauses, mut values) = (Vec::new(), Vec::<Value>::new());
            if let Some(since) = filter.since {
                clauses.push("at >= ?");
                values.push(since.as_unix_ms().into());
            }
            match filter.client {
                Some(DnsClient::Mac(mac)) => {
                    clauses.push("mac = ?");
                    values.push(mac.to_string().into());
                }
                Some(DnsClient::Address(address)) => {
                    clauses.push("client = ? AND mac IS NULL");
                    values.push(address.to_string().into());
                }
                None => {}
            }
            if let Some(outcome) = filter.outcome {
                clauses.push("outcome = ?");
                values.push(enum_text(outcome).into());
            }
            if let Some(text) = filter.domain.filter(|text| !text.is_empty()) {
                clauses.push(r"domain LIKE ? ESCAPE '\'");
                values.push(format!("%{}%", like_escaped(&text)).into());
            }
            if let Some((at, id)) = before {
                clauses.push("(at, id) < (?, ?)");
                values.extend([at.as_unix_ms().into(), id.into()]);
            }
            let filter = if clauses.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", clauses.join(" AND "))
            };
            let limit = limit.min(10_000);
            let sql = format!(
                "SELECT id, at, client, mac, qtype, domain, outcome FROM dns_queries {filter}
                 ORDER BY at DESC, id DESC LIMIT {limit}"
            );
            Ok(conn
                .prepare_cached(&sql)?
                .query_map(params_from_iter(values), |row| {
                    let client: String = row.get(2)?;
                    let mac: Option<String> = row.get(3)?;
                    let query = DnsQuery {
                        at: timestamp_column(row, 1)?,
                        client: client.parse().map_err(|err| invalid(2, err))?,
                        mac: mac
                            .map(|mac| mac.parse())
                            .transpose()
                            .map_err(|err| invalid(3, err))?,
                        qtype: row.get(4)?,
                        domain: row.get(5)?,
                        outcome: enum_column(row, 6)?,
                    };
                    Ok((row.get(0)?, query))
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn dns_totals(&self, since: Timestamp) -> Result<DnsTotals> {
        self.read(move |conn| {
            let hours = conn
                .prepare_cached(
                    "SELECT hour, SUM(queries), SUM(blocked) FROM dns_hours
                     WHERE hour >= ?1 GROUP BY hour ORDER BY hour",
                )?
                .query_map([hour_of(since)], |row| {
                    Ok(DnsHourTotal {
                        hour: timestamp_column(row, 0)?,
                        queries: count_column(row, 1)?,
                        blocked: count_column(row, 2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            let clients = conn
                .prepare_cached(
                    "SELECT client, SUM(queries) AS asked, SUM(blocked) FROM dns_hours
                     WHERE hour >= ?1 GROUP BY client ORDER BY asked DESC, client",
                )?
                .query_map([hour_of(since)], |row| {
                    Ok(DnsClientTotal {
                        client: row.get(0)?,
                        queries: count_column(row, 1)?,
                        blocked: count_column(row, 2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok(DnsTotals { hours, clients })
        })
        .await
    }

    async fn dns_top_domains(
        &self,
        since: Timestamp,
        blocked: bool,
        limit: usize,
    ) -> Result<Vec<DomainCount>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        // `outcome IN (…)` rather than `!=`, so each outcome reads `dns_queries_domains`.
        let outcomes = if blocked {
            vec![Outcome::Blocked]
        } else {
            vec![
                Outcome::Forwarded,
                Outcome::Cached,
                Outcome::Local,
                Outcome::Failed,
            ]
        };
        self.read(move |conn| {
            let marks = vec!["?"; outcomes.len()].join(", ");
            let sql = format!(
                "SELECT domain, COUNT(*) AS asked FROM dns_queries
                 WHERE outcome IN ({marks}) AND at >= ? GROUP BY domain
                 ORDER BY asked DESC, domain LIMIT ?"
            );
            let mut values: Vec<Value> = outcomes.iter().map(|&o| enum_text(o).into()).collect();
            values.extend([since.as_unix_ms().into(), limit.into()]);
            Ok(conn
                .prepare_cached(&sql)?
                .query_map(params_from_iter(values), |row| {
                    Ok(DomainCount {
                        domain: row.get(0)?,
                        queries: count_column(row, 1)?,
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

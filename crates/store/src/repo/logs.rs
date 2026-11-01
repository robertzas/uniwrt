//! [`LogRepo`] on SQLite: the `logs`, `logs_fts`, `log_sources` and `log_searches` tables.

use std::{collections::BTreeMap, time::Duration};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use uniwrt_core::{LogId, NodeId, SearchId, Timestamp, UserId};
use uniwrt_model::log::{
    LogBucket, LogEntry, LogFilter, LogRecord, LogSources, LogVia, NewLog, SavedSearch, Severity,
};

use super::{LogRepo, LogsPrune};
use crate::{
    Error, Result, Store,
    codec::{constraint, timestamp_column},
};

const COLUMNS: &str =
    "id, time, received, node, host, via, seq, facility, severity, program, pid, message, pattern";
/// The most entries one prune deletes, so the writer is never held long.
const PRUNE_BATCH: i64 = 10_000;
/// The newest entries whose size estimates the others'.
const SIZE_SAMPLE: i64 = 1_000;
/// Bytes an entry takes besides its text, which is stored twice (the row and the full-text
/// index): the row's numbers and header, and its two index entries.
const ROW_OVERHEAD: f64 = 96.0;
/// The most words of a text filter that are searched for.
const MAX_TERMS: usize = 16;

impl LogRepo for Store {
    async fn record_logs(&self, logs: Vec<NewLog>) -> Result<Vec<LogRecord>> {
        if logs.is_empty() {
            return Ok(Vec::new());
        }
        self.write(move |conn| {
            let mut insert = conn.prepare_cached(
                "INSERT INTO logs (time, received, node, host, via, seq, facility, severity,
                                   program, pid, message, pattern)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            let mut index = conn.prepare_cached(
                "INSERT INTO logs_fts (rowid, program, message) VALUES (?1, ?2, ?3)",
            )?;
            let mut sources = BTreeMap::<(&str, String), i64>::new();
            let mut records = Vec::with_capacity(logs.len());
            for log in logs {
                let entry = &log.entry;
                insert.execute(params![
                    entry.time.as_unix_ms(),
                    log.received.as_unix_ms(),
                    log.node.map(NodeId::get),
                    log.host,
                    i64::from(log.via == LogVia::Syslog),
                    entry.seq.and_then(|seq| i64::try_from(seq).ok()),
                    entry.facility,
                    entry.severity.code(),
                    entry.program,
                    entry.pid,
                    entry.message,
                    log.pattern,
                ])?;
                let id = conn.last_insert_rowid();
                index.execute(params![id, entry.program, entry.message])?;
                let received = log.received.as_unix_ms();
                for (kind, name) in [("host", &log.host), ("program", &entry.program)] {
                    if let Some(name) = name {
                        let last = sources.entry((kind, name.clone())).or_default();
                        *last = (*last).max(received);
                    }
                }
                records.push(LogRecord::new(LogId::new(id), log));
            }
            let mut source = conn.prepare_cached(
                "INSERT INTO log_sources (kind, name, last_seen) VALUES (?1, ?2, ?3)
                 ON CONFLICT DO UPDATE SET last_seen = max(last_seen, excluded.last_seen)",
            )?;
            for ((kind, name), last_seen) in sources {
                source.execute(params![kind, name, last_seen])?;
            }
            Ok(records)
        })
        .await
    }

    async fn logs(
        &self,
        filter: LogFilter,
        before: Option<(Timestamp, LogId)>,
        limit: usize,
    ) -> Result<Vec<LogRecord>> {
        self.read(move |conn| {
            let mut sql = Where::from(&filter);
            if let Some((time, id)) = before {
                sql.push("(time, id) < (?, ?)", [time.as_unix_ms(), id.get()]);
            }
            let query = format!(
                "SELECT {COLUMNS} FROM logs {} ORDER BY time DESC, id DESC LIMIT {}",
                sql.clause(),
                limit.min(10_000)
            );
            select(conn, &query, sql.values)
        })
        .await
    }

    async fn log_context(&self, id: LogId, around: usize) -> Result<Vec<LogRecord>> {
        self.read(move |conn| {
            let query = format!("SELECT {COLUMNS} FROM logs WHERE id = ?");
            let Some(pivot) = select(conn, &query, vec![Value::Integer(id.get())])?.pop() else {
                return Ok(Vec::new());
            };
            let at = vec![
                Value::Integer(pivot.entry.time.as_unix_ms()),
                Value::Integer(id.get()),
            ];
            let around = around.min(1_000);
            let before = format!(
                "SELECT {COLUMNS} FROM logs WHERE (time, id) < (?, ?)
                 ORDER BY time DESC, id DESC LIMIT {around}"
            );
            let after = format!(
                "SELECT {COLUMNS} FROM logs WHERE (time, id) > (?, ?)
                 ORDER BY time, id LIMIT {around}"
            );
            let mut records = select(conn, &before, at.clone())?;
            records.reverse();
            records.push(pivot);
            records.extend(select(conn, &after, at)?);
            Ok(records)
        })
        .await
    }

    async fn log_histogram(
        &self,
        filter: LogFilter,
        from: Timestamp,
        to: Timestamp,
        step: Duration,
    ) -> Result<Vec<LogBucket>> {
        let step = i64::try_from(step.as_millis()).unwrap_or(i64::MAX).max(1);
        let (from, to) = (from.as_unix_ms(), to.as_unix_ms().max(from.as_unix_ms()));
        self.read(move |conn| {
            let mut sql = Where::from(&filter);
            sql.push("time >= ? AND time < ?", [from, to]);
            let query = format!(
                "SELECT (time - {from}) / {step}, severity, count(*) FROM logs {}
                 GROUP BY 1, 2",
                sql.clause()
            );
            let buckets = usize::try_from((to - from + step - 1) / step).unwrap_or(0);
            let mut histogram: Vec<_> = (0..buckets)
                .map(|index| LogBucket {
                    start: Timestamp::from_unix_ms(
                        from.saturating_add(step.saturating_mul(i64::try_from(index).unwrap_or(0))),
                    ),
                    counts: [0; 8],
                })
                .collect();
            let mut statement = conn.prepare(&query)?;
            let rows = statement.query_map(params_from_iter(sql.values), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, u8>(1)?,
                    row.get::<_, u32>(2)?,
                ))
            })?;
            for row in rows {
                let (bucket, severity, count) = row?;
                if let Some(bucket) = usize::try_from(bucket)
                    .ok()
                    .and_then(|b| histogram.get_mut(b))
                {
                    bucket.counts[usize::from(severity.min(7))] += count;
                }
            }
            Ok(histogram)
        })
        .await
    }

    async fn last_log(&self, node: NodeId) -> Result<Option<LogRecord>> {
        self.read(move |conn| {
            let query =
                format!("SELECT {COLUMNS} FROM logs WHERE node = ? ORDER BY id DESC LIMIT 1");
            Ok(select(conn, &query, vec![Value::Integer(node.get())])?.pop())
        })
        .await
    }

    async fn log_sources(&self, since: Timestamp) -> Result<LogSources> {
        self.read(move |conn| {
            let mut statement = conn.prepare_cached(
                "SELECT kind, name FROM log_sources WHERE last_seen >= ?1 ORDER BY name",
            )?;
            let rows = statement.query_map([since.as_unix_ms()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut sources = LogSources::default();
            for row in rows {
                let (kind, name) = row?;
                match kind.as_str() {
                    "host" => sources.hosts.push(name),
                    _ => sources.programs.push(name),
                }
            }
            Ok(sources)
        })
        .await
    }

    async fn prune_logs(&self, prune: LogsPrune) -> Result<usize> {
        self.write(move |conn| {
            let (Some(first), Some(last)) =
                conn.query_row("SELECT min(id), max(id) FROM logs", [], |row| {
                    Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
                })?
            else {
                return Ok(0);
            };
            // Ids grow in arrival order: the first that arrived in time, found by walking up
            // from the oldest (over exactly the entries to delete).
            let in_time = conn
                .query_row(
                    "SELECT id FROM logs WHERE received >= ?1 ORDER BY id LIMIT 1",
                    [prune.before.as_unix_ms()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .unwrap_or(last + 1);
            let text: Option<f64> = conn.query_row(
                "SELECT avg(length(message) + coalesce(length(program), 0)
                            + coalesce(length(host), 0) + coalesce(length(pattern), 0))
                 FROM (SELECT message, program, host, pattern FROM logs ORDER BY id DESC LIMIT ?1)",
                [SIZE_SAMPLE],
                |row| row.get(0),
            )?;
            let per_entry = 2.0f64.mul_add(text.unwrap_or(0.0), ROW_OVERHEAD);
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let fit = (prune.max_bytes as f64 / per_entry) as i64;
            let in_size = last.saturating_sub(fit) + 1;
            let keep_from = in_time.max(in_size).min(first.saturating_add(PRUNE_BATCH));
            let deleted = if keep_from > first {
                conn.execute("DELETE FROM logs WHERE id < ?1", [keep_from])?
            } else {
                0
            };
            conn.execute(
                "DELETE FROM log_sources WHERE last_seen < ?1",
                [prune.before.as_unix_ms()],
            )?;
            Ok(deleted)
        })
        .await
    }

    async fn saved_searches(&self) -> Result<Vec<SavedSearch>> {
        self.read(|conn| {
            let mut statement = conn.prepare_cached(
                "SELECT id, name, filter, created_by, created_at FROM log_searches ORDER BY name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    SearchId::new(row.get(0)?),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?.map(UserId::new),
                    timestamp_column(row, 4)?,
                ))
            })?;
            let mut searches = Vec::new();
            for row in rows {
                let (id, name, filter, created_by, created_at) = row?;
                let filter = serde_json::from_str(&filter).map_err(|source| Error::Decode {
                    what: "saved search",
                    key: id.to_string(),
                    source,
                })?;
                searches.push(SavedSearch {
                    id,
                    name,
                    filter,
                    created_by,
                    created_at,
                });
            }
            Ok(searches)
        })
        .await
    }

    async fn save_search(
        &self,
        name: String,
        filter: LogFilter,
        by: Option<UserId>,
    ) -> Result<SavedSearch> {
        let json = serde_json::to_string(&filter).map_err(|source| Error::Encode {
            what: "saved search",
            source,
        })?;
        let created_at = self.now();
        self.write(move |conn| {
            conn.execute(
                "INSERT INTO log_searches (name, filter, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, json, by.map(UserId::get), created_at.as_unix_ms()],
            )
            .map_err(|err| constraint(err, "saved search", &name))?;
            Ok(SavedSearch {
                id: SearchId::new(conn.last_insert_rowid()),
                name,
                filter,
                created_by: by,
                created_at,
            })
        })
        .await
    }

    async fn delete_search(&self, id: SearchId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn.execute("DELETE FROM log_searches WHERE id = ?1", [id.get()])? > 0)
        })
        .await
    }
}

/// A WHERE clause and its values, in order.
#[derive(Default)]
struct Where {
    clauses: Vec<String>,
    values: Vec<Value>,
}

impl Where {
    fn push<V: Into<Value>>(
        &mut self,
        clause: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) {
        self.clauses.push(clause.into());
        self.values.extend(values.into_iter().map(Into::into));
    }

    /// `column IN (?, …)` over the values, when there are any.
    fn any<V: Into<Value>>(&mut self, column: &str, values: impl ExactSizeIterator<Item = V>) {
        if values.len() > 0 {
            let marks = vec!["?"; values.len()].join(", ");
            self.push(format!("{column} IN ({marks})"), values);
        }
    }

    fn clause(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

impl From<&LogFilter> for Where {
    fn from(filter: &LogFilter) -> Self {
        let mut sql = Self::default();
        sql.any("node", filter.nodes.iter().map(|node| node.get()));
        sql.any("host", filter.hosts.iter().cloned());
        sql.any("program", filter.programs.iter().cloned());
        if let Some(min) = filter.min {
            sql.push("severity <= ?", [i64::from(min.code())]);
        }
        if filter.annotated {
            sql.clauses.push("pattern IS NOT NULL".to_owned());
        }
        if let Some(from) = filter.from {
            sql.push("time >= ?", [from.as_unix_ms()]);
        }
        if let Some(to) = filter.to {
            sql.push("time < ?", [to.as_unix_ms()]);
        }
        if let Some(text) = &filter.text {
            let (wanted, unwanted) = fts_terms(text);
            if !wanted.is_empty() {
                sql.push(
                    "id IN (SELECT rowid FROM logs_fts WHERE logs_fts MATCH ?)",
                    [wanted.join(" ")],
                );
            }
            if !unwanted.is_empty() {
                sql.push(
                    "id NOT IN (SELECT rowid FROM logs_fts WHERE logs_fts MATCH ?)",
                    [unwanted.join(" OR ")],
                );
            }
        }
        sql
    }
}

/// A text filter as FTS5 phrases: the words and `"phrases"` that must appear, and those
/// (`-word`) that must not. A word ending in `*` is a prefix. Words without a letter or digit
/// are skipped, as the index has no tokens for them.
pub(super) fn fts_terms(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut wanted, mut unwanted) = (Vec::new(), Vec::new());
    let mut rest = text.trim_start();
    while !rest.is_empty() && wanted.len() + unwanted.len() < MAX_TERMS {
        let negated = rest.starts_with('-');
        if negated {
            rest = &rest[1..];
        }
        let (word, prefix, after) = if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            (&quoted[..end], false, quoted.get(end + 1..).unwrap_or(""))
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let word = &rest[..end];
            let stem = word.trim_end_matches('*');
            (stem, stem.len() < word.len(), &rest[end..])
        };
        rest = after.trim_start();
        if !word.chars().any(char::is_alphanumeric) {
            continue;
        }
        let phrase = format!(
            "\"{}\"{}",
            word.replace('"', "\"\""),
            if prefix { " *" } else { "" }
        );
        if negated { &mut unwanted } else { &mut wanted }.push(phrase);
    }
    (wanted, unwanted)
}

fn select(conn: &Connection, query: &str, values: Vec<Value>) -> Result<Vec<LogRecord>> {
    // Filters make many shapes of query: not worth caching.
    let mut statement = conn.prepare(query)?;
    let rows = statement.query_map(params_from_iter(values), record)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn record(row: &Row<'_>) -> rusqlite::Result<LogRecord> {
    Ok(LogRecord {
        id: LogId::new(row.get(0)?),
        received: timestamp_column(row, 2)?,
        node: row.get::<_, Option<i64>>(3)?.map(NodeId::new),
        host: row.get(4)?,
        via: if row.get::<_, i64>(5)? == 1 {
            LogVia::Syslog
        } else {
            LogVia::Agent
        },
        entry: LogEntry {
            seq: row
                .get::<_, Option<i64>>(6)?
                .and_then(|seq| u64::try_from(seq).ok()),
            time: timestamp_column(row, 1)?,
            facility: row.get(7)?,
            severity: Severity::from_code(row.get(8)?),
            program: row.get(9)?,
            pid: row.get(10)?,
            message: row.get(11)?,
        },
        pattern: row.get(12)?,
    })
}

#[cfg(test)]
#[path = "logs_tests.rs"]
mod tests;

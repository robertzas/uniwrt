//! [`EventRepo`] on SQLite: the `events` table.

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use uniwrt_core::{EventId, NodeId, Timestamp};
use uniwrt_model::events::{
    Condition, EventFilter, EventKind, EventLevel, EventRecord, EventWrite, NewEvent,
};

use super::{EventRepo, EventsPrune};
use crate::{
    Error, Result, Store,
    codec::{enum_column, enum_text, timestamp_column},
};

const COLUMNS: &str =
    "id, at, last_at, count, level, category, node, client, cond_key, cond_raised, what";
/// The most events one prune deletes, so the writer is never held long.
const PRUNE_BATCH: i64 = 10_000;

impl EventRepo for Store {
    async fn record_events(&self, writes: Vec<EventWrite>) -> Result<Vec<Option<EventRecord>>> {
        if writes.is_empty() {
            return Ok(Vec::new());
        }
        let encoded = writes
            .into_iter()
            .map(|write| match write {
                EventWrite::Insert(event) => serde_json::to_string(&event.what)
                    .map(|what| (EventWrite::Insert(event), Some(what)))
                    .map_err(|source| Error::Encode {
                        what: "event",
                        source,
                    }),
                fold @ EventWrite::Fold { .. } => Ok((fold, None)),
            })
            .collect::<Result<Vec<_>>>()?;
        self.write(move |conn| {
            let mut records = Vec::with_capacity(encoded.len());
            for (write, what) in encoded {
                records.push(match write {
                    EventWrite::Insert(event) => {
                        Some(insert(conn, event, &what.unwrap_or_default())?)
                    }
                    EventWrite::Fold { id, last_at, count } => {
                        let query = format!(
                            "UPDATE events SET last_at = max(last_at, ?1), count = count + ?2
                             WHERE id = ?3 RETURNING {COLUMNS}"
                        );
                        conn.prepare_cached(&query)?
                            .query_row(params![last_at.as_unix_ms(), count, id.get()], record)
                            .optional()?
                    }
                });
            }
            Ok(records)
        })
        .await
    }

    async fn events(
        &self,
        filter: EventFilter,
        before: Option<(Timestamp, EventId)>,
        limit: usize,
    ) -> Result<Vec<EventRecord>> {
        self.read(move |conn| {
            let mut sql = Where::from(&filter);
            if let Some((at, id)) = before {
                sql.push("(at, id) < (?, ?)", [at.as_unix_ms(), id.get()]);
            }
            let query = format!(
                "SELECT {COLUMNS} FROM events {} ORDER BY at DESC, id DESC LIMIT {}",
                sql.clause(),
                limit.min(10_000)
            );
            select(conn, &query, sql.values)
        })
        .await
    }

    async fn raised_conditions(&self) -> Result<Vec<(String, Timestamp)>> {
        self.read(|conn| {
            let mut statement = conn.prepare_cached(
                "SELECT cond_key, at FROM events
                 WHERE id IN (SELECT max(id) FROM events WHERE cond_key IS NOT NULL
                              GROUP BY cond_key)
                   AND cond_raised = 1
                 ORDER BY cond_key",
            )?;
            let rows =
                statement.query_map([], |row| Ok((row.get(0)?, timestamp_column(row, 1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn prune_events(&self, prune: EventsPrune) -> Result<usize> {
        self.write(move |conn| {
            let (Some(first), Some(last)) =
                conn.query_row("SELECT min(id), max(id) FROM events", [], |row| {
                    Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
                })?
            else {
                return Ok(0);
            };
            // Ids grow in recording order: the first one recent enough, walking up from the
            // oldest (over the events to delete).
            let in_time = conn
                .query_row(
                    "SELECT id FROM events WHERE last_at >= ?1 ORDER BY id LIMIT 1",
                    [prune.before.as_unix_ms()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .unwrap_or(last + 1);
            let in_count = last
                .saturating_sub(i64::try_from(prune.max_events).unwrap_or(i64::MAX))
                .saturating_add(1);
            let keep_from = in_time.max(in_count).min(first.saturating_add(PRUNE_BATCH));
            Ok(if keep_from > first {
                conn.execute("DELETE FROM events WHERE id < ?1", [keep_from])?
            } else {
                0
            })
        })
        .await
    }
}

fn insert(conn: &Connection, event: NewEvent, what: &str) -> Result<EventRecord> {
    let (key, raised) = event.condition.as_ref().map_or((None, None), |condition| {
        (Some(condition.key.as_str()), Some(condition.raised))
    });
    conn.prepare_cached(
        "INSERT INTO events (at, last_at, count, level, category, kind, node, client, cond_key,
                             cond_raised, what)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?
    .execute(params![
        event.at.as_unix_ms(),
        event.last_at.as_unix_ms(),
        event.count,
        event.level.code(),
        enum_text(event.category),
        event.what.name(),
        event.node.map(NodeId::get),
        event.client.map(|mac| mac.to_string()),
        key,
        raised,
        what,
    ])?;
    let id = EventId::new(conn.last_insert_rowid());
    Ok(EventRecord::new(id, event))
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

impl From<&EventFilter> for Where {
    fn from(filter: &EventFilter) -> Self {
        let mut sql = Self::default();
        sql.any("category", filter.categories.iter().map(enum_text));
        sql.any("kind", filter.kinds.iter().cloned());
        if let Some(min) = filter.min {
            sql.push("level <= ?", [i64::from(min.code())]);
        }
        sql.any("node", filter.nodes.iter().map(|node| node.get()));
        sql.any("client", filter.clients.iter().map(ToString::to_string));
        if let Some(from) = filter.from {
            sql.push("at >= ?", [from.as_unix_ms()]);
        }
        if let Some(to) = filter.to {
            sql.push("at < ?", [to.as_unix_ms()]);
        }
        sql
    }
}

fn select(conn: &Connection, query: &str, values: Vec<Value>) -> Result<Vec<EventRecord>> {
    // Filters make many shapes of query: not worth caching.
    let mut statement = conn.prepare(query)?;
    let rows = statement.query_map(params_from_iter(values), record)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn record(row: &Row<'_>) -> rusqlite::Result<EventRecord> {
    let what: String = row.get(10)?;
    let what: EventKind = serde_json::from_str(&what).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let condition = row
        .get::<_, Option<String>>(8)?
        .map(|key| -> rusqlite::Result<Condition> {
            Ok(Condition {
                key,
                raised: row.get::<_, Option<bool>>(9)?.unwrap_or(false),
            })
        })
        .transpose()?;
    Ok(EventRecord {
        id: EventId::new(row.get(0)?),
        at: timestamp_column(row, 1)?,
        last_at: timestamp_column(row, 2)?,
        count: row.get(3)?,
        level: EventLevel::from_code(row.get(4)?),
        category: enum_column(row, 5)?,
        node: row.get::<_, Option<i64>>(6)?.map(NodeId::new),
        client: row
            .get::<_, Option<String>>(7)?
            .map(|mac| {
                mac.parse().map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        format!("{mac:?} is not a MAC address").into(),
                    )
                })
            })
            .transpose()?,
        condition,
        what,
    })
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

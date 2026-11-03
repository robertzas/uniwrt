//! [`AlertRepo`] on SQLite: the `alert_rules`, `alerts` and `maintenance_windows` tables.

use rusqlite::{Connection, OptionalExtension as _, Row, params, params_from_iter, types::Value};
use serde::{Serialize, de::DeserializeOwned};
use uniwrt_core::{AlertId, NodeId, RuleId, Timestamp, UserId, WindowId};
use uniwrt_model::alerts::{
    Acknowledgement, AlertFilter, AlertRecord, AlertRule, AlertState, AlertWrite,
    MaintenanceWindow, NewWindow, RuleSpec,
};

use super::AlertRepo;
use crate::{Error, Result, Store, codec::timestamp_column};

const ALERT_COLUMNS: &str = "id, rule_id, state, ack_by, ack_name, ack_at";
const RULE_COLUMNS: &str = "id, preset, spec";
const WINDOW_COLUMNS: &str = "id, starts_at, ends_at, nodes, note, created_by";
/// The most alerts one prune deletes, so the writer is never held long.
const PRUNE_BATCH: i64 = 10_000;

impl AlertRepo for Store {
    async fn alert_rules(&self) -> Result<Vec<AlertRule>> {
        self.read(|conn| {
            let query = format!("SELECT {RULE_COLUMNS} FROM alert_rules ORDER BY id");
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([], rule)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn alert_rule(&self, id: RuleId) -> Result<Option<AlertRule>> {
        self.read(move |conn| {
            let query = format!("SELECT {RULE_COLUMNS} FROM alert_rules WHERE id = ?1");
            Ok(conn
                .prepare_cached(&query)?
                .query_row([id.get()], rule)
                .optional()?)
        })
        .await
    }

    async fn seed_alert_rules(&self, presets: Vec<(String, RuleSpec)>) -> Result<usize> {
        let encoded = presets
            .into_iter()
            .map(|(preset, spec)| Ok((preset, encode(&spec, "alert rule")?)))
            .collect::<Result<Vec<_>>>()?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let mut added = 0;
            for (preset, spec) in encoded {
                added += conn
                    .prepare_cached(
                        "INSERT INTO alert_rules (preset, spec, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?3) ON CONFLICT (preset) DO NOTHING",
                    )?
                    .execute(params![preset, spec, now])?;
            }
            Ok(added)
        })
        .await
    }

    async fn create_alert_rule(&self, spec: RuleSpec) -> Result<AlertRule> {
        let encoded = encode(&spec, "alert rule")?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO alert_rules (spec, created_at, updated_at) VALUES (?1, ?2, ?2)",
            )?
            .execute(params![encoded, now])?;
            Ok(AlertRule {
                id: RuleId::new(conn.last_insert_rowid()),
                preset: None,
                spec,
            })
        })
        .await
    }

    async fn update_alert_rule(&self, id: RuleId, spec: RuleSpec) -> Result<Option<AlertRule>> {
        let encoded = encode(&spec, "alert rule")?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let query = format!(
                "UPDATE alert_rules SET spec = ?2, updated_at = ?3 WHERE id = ?1
                 RETURNING {RULE_COLUMNS}"
            );
            Ok(conn
                .prepare_cached(&query)?
                .query_row(params![id.get(), encoded, now], rule)
                .optional()?)
        })
        .await
    }

    async fn delete_alert_rule(&self, id: RuleId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM alert_rules WHERE id = ?1 AND preset IS NULL")?
                .execute([id.get()])?
                > 0)
        })
        .await
    }

    async fn record_alerts(&self, writes: Vec<AlertWrite>) -> Result<Vec<AlertRecord>> {
        if writes.is_empty() {
            return Ok(Vec::new());
        }
        let encoded = writes
            .into_iter()
            .map(|write| {
                let (kind, rule, state) = match write {
                    AlertWrite::Open { rule, state } => (Kind::Open, rule, state),
                    AlertWrite::Update { rule, state } => (Kind::Update, rule, state),
                    AlertWrite::Reopen { rule, state } => (Kind::Reopen, rule, state),
                };
                let json = encode(&state, "alert")?;
                Ok((kind, rule, state, json))
            })
            .collect::<Result<Vec<_>>>()?;
        self.write(move |conn| {
            let mut stored = Vec::with_capacity(encoded.len());
            for (kind, rule, state, json) in encoded {
                let record = match kind {
                    Kind::Open => Some(insert(conn, rule, &state, &json)?),
                    Kind::Update | Kind::Reopen => update(conn, rule, &state, &json, kind)?,
                };
                stored.extend(record);
            }
            Ok(stored)
        })
        .await
    }

    async fn alerts(
        &self,
        filter: AlertFilter,
        before: Option<(Timestamp, AlertId)>,
        limit: usize,
    ) -> Result<Vec<AlertRecord>> {
        self.read(move |conn| {
            let mut clauses = Vec::new();
            let mut values: Vec<Value> = Vec::new();
            if filter.active {
                clauses.push("resolved_at IS NULL".to_owned());
            }
            if let Some(min) = filter.min {
                clauses.push("level <= ?".to_owned());
                values.push(i64::from(min.code()).into());
            }
            let mut any = |column: &str, ids: Vec<i64>| {
                if !ids.is_empty() {
                    clauses.push(format!("{column} IN ({})", vec!["?"; ids.len()].join(", ")));
                    values.extend(ids.into_iter().map(Value::from));
                }
            };
            any("node", filter.nodes.iter().map(|node| node.get()).collect());
            any(
                "rule_id",
                filter.rules.iter().map(|rule| rule.get()).collect(),
            );
            let mut bound = |clause: &str, value: i64| {
                clauses.push(clause.to_owned());
                values.push(value.into());
            };
            if let Some(from) = filter.from {
                bound("started_at >= ?", from.as_unix_ms());
            }
            if let Some(to) = filter.to {
                bound("started_at < ?", to.as_unix_ms());
            }
            if let Some((at, id)) = before {
                clauses.push("(started_at, id) < (?, ?)".to_owned());
                values.extend([Value::from(at.as_unix_ms()), Value::from(id.get())]);
            }
            let clause = if clauses.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", clauses.join(" AND "))
            };
            let query = format!(
                "SELECT {ALERT_COLUMNS} FROM alerts {clause}
                 ORDER BY started_at DESC, id DESC LIMIT {}",
                limit.min(10_000)
            );
            // Filters make many shapes of query: not worth caching.
            let mut statement = conn.prepare(&query)?;
            let rows = statement.query_map(params_from_iter(values), alert)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn alert(&self, id: AlertId) -> Result<Option<AlertRecord>> {
        self.read(move |conn| {
            let query = format!("SELECT {ALERT_COLUMNS} FROM alerts WHERE id = ?1");
            Ok(conn
                .prepare_cached(&query)?
                .query_row([id.get()], alert)
                .optional()?)
        })
        .await
    }

    async fn alerts_to_resume(&self, since: Timestamp) -> Result<Vec<AlertRecord>> {
        self.read(move |conn| {
            let query = format!(
                "SELECT {ALERT_COLUMNS} FROM alerts
                 WHERE id IN (SELECT max(id) FROM alerts GROUP BY rule_id, subject)
                   AND (resolved_at IS NULL OR resolved_at >= ?1)
                 ORDER BY id"
            );
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([since.as_unix_ms()], alert)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn acknowledge_alert(
        &self,
        id: AlertId,
        ack: Acknowledgement,
    ) -> Result<Option<AlertRecord>> {
        self.write(move |conn| {
            let query = format!(
                "UPDATE alerts SET ack_by = ?2, ack_name = ?3, ack_at = ?4
                 WHERE id = ?1 AND resolved_at IS NULL RETURNING {ALERT_COLUMNS}"
            );
            Ok(conn
                .prepare_cached(&query)?
                .query_row(
                    params![id.get(), ack.by.get(), ack.username, ack.at.as_unix_ms()],
                    alert,
                )
                .optional()?)
        })
        .await
    }

    async fn prune_alerts(&self, before: Timestamp) -> Result<usize> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached(
                    "DELETE FROM alerts WHERE id IN (
                        SELECT id FROM alerts WHERE resolved_at < ?1 LIMIT ?2)",
                )?
                .execute(params![before.as_unix_ms(), PRUNE_BATCH])?)
        })
        .await
    }

    async fn maintenance_windows(&self, after: Timestamp) -> Result<Vec<MaintenanceWindow>> {
        self.read(move |conn| {
            let query = format!(
                "SELECT {WINDOW_COLUMNS} FROM maintenance_windows WHERE ends_at > ?1
                 ORDER BY starts_at, id"
            );
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([after.as_unix_ms()], window)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn create_maintenance_window(
        &self,
        new: NewWindow,
        created_by: Option<UserId>,
    ) -> Result<MaintenanceWindow> {
        let nodes = encode(&new.nodes, "maintenance window")?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO maintenance_windows
                     (starts_at, ends_at, nodes, note, created_by, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                new.starts_at.as_unix_ms(),
                new.ends_at.as_unix_ms(),
                nodes,
                new.note,
                created_by.map(UserId::get),
                now
            ])?;
            Ok(MaintenanceWindow {
                id: WindowId::new(conn.last_insert_rowid()),
                starts_at: new.starts_at,
                ends_at: new.ends_at,
                nodes: new.nodes,
                note: new.note,
                created_by,
            })
        })
        .await
    }

    async fn delete_maintenance_window(&self, id: WindowId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM maintenance_windows WHERE id = ?1")?
                .execute([id.get()])?
                > 0)
        })
        .await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Open,
    Update,
    Reopen,
}

fn insert(conn: &Connection, rule: RuleId, state: &AlertState, json: &str) -> Result<AlertRecord> {
    conn.prepare_cached(
        "INSERT INTO alerts (rule_id, subject, level, node, started_at, resolved_at, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        rule.get(),
        state.subject,
        state.level.code(),
        state.node.map(NodeId::get),
        state.started_at.as_unix_ms(),
        state.resolved_at.map(Timestamp::as_unix_ms),
        json,
    ])?;
    Ok(AlertRecord {
        id: AlertId::new(conn.last_insert_rowid()),
        rule,
        state: state.clone(),
        acknowledged: None,
    })
}

/// The latest alert of the rule and subject, as `state` says (its acknowledgement gone on a
/// reopen).
fn update(
    conn: &Connection,
    rule: RuleId,
    state: &AlertState,
    json: &str,
    kind: Kind,
) -> Result<Option<AlertRecord>> {
    let ack = if kind == Kind::Reopen {
        ", ack_by = NULL, ack_name = NULL, ack_at = NULL"
    } else {
        ""
    };
    let query = format!(
        "UPDATE alerts SET level = ?3, node = ?4, started_at = ?5, resolved_at = ?6, state = ?7
             {ack}
         WHERE id = (SELECT max(id) FROM alerts WHERE rule_id = ?1 AND subject = ?2)
         RETURNING {ALERT_COLUMNS}"
    );
    Ok(conn
        .prepare_cached(&query)?
        .query_row(
            params![
                rule.get(),
                state.subject,
                state.level.code(),
                state.node.map(NodeId::get),
                state.started_at.as_unix_ms(),
                state.resolved_at.map(Timestamp::as_unix_ms),
                json,
            ],
            alert,
        )
        .optional()?)
}

fn encode(value: &impl Serialize, what: &'static str) -> Result<String> {
    serde_json::to_string(value).map_err(|source| Error::Encode { what, source })
}

fn json_column<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn rule(row: &Row<'_>) -> rusqlite::Result<AlertRule> {
    Ok(AlertRule {
        id: RuleId::new(row.get(0)?),
        preset: row.get(1)?,
        spec: json_column(row, 2)?,
    })
}

fn alert(row: &Row<'_>) -> rusqlite::Result<AlertRecord> {
    let acknowledged = match row.get::<_, Option<i64>>(3)? {
        Some(by) => Some(Acknowledgement {
            by: UserId::new(by),
            username: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            at: timestamp_column(row, 5)?,
        }),
        None => None,
    };
    Ok(AlertRecord {
        id: AlertId::new(row.get(0)?),
        rule: RuleId::new(row.get(1)?),
        state: json_column(row, 2)?,
        acknowledged,
    })
}

fn window(row: &Row<'_>) -> rusqlite::Result<MaintenanceWindow> {
    Ok(MaintenanceWindow {
        id: WindowId::new(row.get(0)?),
        starts_at: timestamp_column(row, 1)?,
        ends_at: timestamp_column(row, 2)?,
        nodes: json_column(row, 3)?,
        note: row.get(4)?,
        created_by: row.get::<_, Option<i64>>(5)?.map(UserId::new),
    })
}

#[cfg(test)]
#[path = "alerts_tests.rs"]
mod tests;

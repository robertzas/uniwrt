//! [`NotifierRepo`] and [`SettingsRepo`] on SQLite: the `notifiers` and `settings` tables.

use rusqlite::{OptionalExtension as _, Row, params};
use uniwrt_core::{NotifierId, Timestamp};
use uniwrt_model::notify::{DeliveryStatus, Notifier, NotifierConfig};

use super::{NotifierRepo, SettingsRepo};
use crate::{Error, Result, Store, codec::timestamp_column};

const COLUMNS: &str = "id, name, enabled, spec, filter, created_at, updated_at, last_ok_at, \
                       last_error_at, last_error";

impl NotifierRepo for Store {
    async fn notifiers(&self) -> Result<Vec<Notifier>> {
        self.read(|conn| {
            let query = format!("SELECT {COLUMNS} FROM notifiers ORDER BY id");
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([], notifier)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn notifier(&self, id: NotifierId) -> Result<Option<Notifier>> {
        self.read(move |conn| {
            let query = format!("SELECT {COLUMNS} FROM notifiers WHERE id = ?1");
            Ok(conn
                .prepare_cached(&query)?
                .query_row([id.get()], notifier)
                .optional()?)
        })
        .await
    }

    async fn create_notifier(&self, config: NotifierConfig) -> Result<Notifier> {
        let (spec, filter) = encode(&config)?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let query = format!(
                "INSERT INTO notifiers (name, enabled, spec, filter, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5) RETURNING {COLUMNS}"
            );
            Ok(conn.prepare_cached(&query)?.query_row(
                params![config.name, config.enabled, spec, filter, now],
                notifier,
            )?)
        })
        .await
    }

    async fn update_notifier(
        &self,
        id: NotifierId,
        config: NotifierConfig,
    ) -> Result<Option<Notifier>> {
        let (spec, filter) = encode(&config)?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let query = format!(
                "UPDATE notifiers SET name = ?2, enabled = ?3, spec = ?4, filter = ?5,
                     updated_at = ?6
                 WHERE id = ?1 RETURNING {COLUMNS}"
            );
            Ok(conn
                .prepare_cached(&query)?
                .query_row(
                    params![id.get(), config.name, config.enabled, spec, filter, now],
                    notifier,
                )
                .optional()?)
        })
        .await
    }

    async fn delete_notifier(&self, id: NotifierId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM notifiers WHERE id = ?1")?
                .execute([id.get()])?
                > 0)
        })
        .await
    }

    async fn record_delivery(
        &self,
        id: NotifierId,
        at: Timestamp,
        error: Option<String>,
    ) -> Result<()> {
        let at = at.as_unix_ms();
        self.write(move |conn| {
            match error {
                None => conn
                    .prepare_cached("UPDATE notifiers SET last_ok_at = ?2 WHERE id = ?1")?
                    .execute(params![id.get(), at])?,
                Some(error) => conn
                    .prepare_cached(
                        "UPDATE notifiers SET last_error_at = ?2, last_error = ?3 WHERE id = ?1",
                    )?
                    .execute(params![id.get(), at, error])?,
            };
            Ok(())
        })
        .await
    }
}

impl SettingsRepo for Store {
    async fn setting(&self, key: &'static str) -> Result<Option<serde_json::Value>> {
        self.read(move |conn| {
            let text: Option<String> = conn
                .prepare_cached("SELECT value FROM settings WHERE key = ?1")?
                .query_row([key], |row| row.get(0))
                .optional()?;
            text.map(|text| {
                serde_json::from_str(&text).map_err(|source| Error::Decode {
                    what: "setting",
                    key: key.to_owned(),
                    source,
                })
            })
            .transpose()
        })
        .await
    }

    async fn put_setting(&self, key: &'static str, value: serde_json::Value) -> Result<()> {
        let text = serde_json::to_string(&value).map_err(|source| Error::Encode {
            what: "setting",
            source,
        })?;
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value,
                     updated_at = excluded.updated_at",
            )?
            .execute(params![key, text, now])?;
            Ok(())
        })
        .await
    }
}

fn encode(config: &NotifierConfig) -> Result<(String, String)> {
    let error = |source| Error::Encode {
        what: "notifier",
        source,
    };
    Ok((
        serde_json::to_string(&config.spec).map_err(error)?,
        serde_json::to_string(&config.filter).map_err(error)?,
    ))
}

fn json_column<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn notifier(row: &Row<'_>) -> rusqlite::Result<Notifier> {
    let optional_time = |index| {
        row.get::<_, Option<i64>>(index)
            .map(|ms| ms.map(Timestamp::from_unix_ms))
    };
    Ok(Notifier {
        id: NotifierId::new(row.get(0)?),
        config: NotifierConfig {
            name: row.get(1)?,
            enabled: row.get(2)?,
            spec: json_column(row, 3)?,
            filter: json_column(row, 4)?,
        },
        created_at: timestamp_column(row, 5)?,
        updated_at: timestamp_column(row, 6)?,
        status: DeliveryStatus {
            last_ok_at: optional_time(7)?,
            last_error_at: optional_time(8)?,
            last_error: row.get(9)?,
        },
    })
}

#[cfg(test)]
#[path = "notifiers_tests.rs"]
mod tests;

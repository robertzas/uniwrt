//! [`AuditRepo`] on SQLite: the `audit` table.

use rusqlite::params;
use uniwrt_core::UserId;
use uniwrt_model::user::AuditEntry;

use super::AuditRepo;
use crate::{
    Result, Store,
    codec::{enum_column, enum_text, timestamp_column},
};

impl AuditRepo for Store {
    async fn record(&self, entry: AuditEntry) -> Result<()> {
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO audit (at, action, user_id, username, ip, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                entry.at.as_unix_ms(),
                enum_text(entry.action),
                entry.user_id.map(UserId::get),
                entry.username,
                entry.ip,
                entry.detail
            ])?;
            Ok(())
        })
        .await
    }

    async fn audit_log(&self, limit: usize) -> Result<Vec<AuditEntry>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT at, action, user_id, username, ip, detail FROM audit
                     ORDER BY id DESC LIMIT ?1",
                )?
                .query_map([limit], |row| {
                    Ok(AuditEntry {
                        at: timestamp_column(row, 0)?,
                        action: enum_column(row, 1)?,
                        user_id: row.get::<_, Option<i64>>(2)?.map(UserId::new),
                        username: row.get(3)?,
                        ip: row.get(4)?,
                        detail: row.get(5)?,
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

    use uniwrt_model::user::{AuditAction, AuditEntry};

    use crate::{AuditRepo as _, testing};

    #[tokio::test]
    async fn entries_come_back_newest_first() {
        let (_dir, store, _) = testing::open().await;
        let entry = |secs, action, username: Option<&str>| AuditEntry {
            at: testing::START.saturating_add(Duration::from_secs(secs)),
            action,
            user_id: None,
            username: username.map(str::to_owned),
            ip: Some("192.168.1.20".into()),
            detail: None,
        };
        let entries = [
            entry(0, AuditAction::LoginFailed, Some("root")),
            entry(1, AuditAction::Login, Some("admin")),
            AuditEntry {
                detail: Some("root@10.0.0.2:22, job 1".into()),
                ..entry(2, AuditAction::Adopt, Some("admin"))
            },
            entry(3, AuditAction::Logout, None),
        ];
        for entry in entries.clone() {
            store.record(entry).await.unwrap();
        }
        let log = store.audit_log(2).await.unwrap();
        assert_eq!(log, [entries[3].clone(), entries[2].clone()]);
    }
}

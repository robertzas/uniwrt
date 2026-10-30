//! [`SessionRepo`] on SQLite: the `sessions` table.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::Timestamp;
use uniwrt_model::user::Session;

use super::{NewSession, SessionRepo, users};
use crate::{
    Result, Store,
    codec::{constraint, timestamp_column},
};

impl SessionRepo for Store {
    async fn create_session(&self, session: NewSession) -> Result<()> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let NewSession {
                token_sha256,
                user,
                expires_at,
                ip,
                user_agent,
            } = session;
            conn.prepare_cached(
                "INSERT INTO sessions
                 (token_sha256, user_id, created_at, last_seen_at, expires_at, ip, user_agent)
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                token_sha256,
                user.get(),
                now,
                expires_at.as_unix_ms(),
                ip,
                user_agent
            ])
            .map_err(|err| constraint(err, "user", user))?;
            Ok(())
        })
        .await
    }

    async fn session(&self, token_sha256: [u8; 32]) -> Result<Option<Session>> {
        self.read(move |conn| {
            let columns = users::COLUMNS.replace(", ", ", u.");
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT s.created_at, s.last_seen_at, s.expires_at, u.{columns}
                     FROM sessions s JOIN users u ON u.id = s.user_id
                     WHERE s.token_sha256 = ?1"
                ))?
                .query_row([token_sha256], |row| {
                    Ok(Session {
                        created_at: timestamp_column(row, 0)?,
                        last_seen_at: timestamp_column(row, 1)?,
                        expires_at: timestamp_column(row, 2)?,
                        user: users::user_from_row(row, 3)?,
                    })
                })
                .optional()?)
        })
        .await
    }

    async fn touch_session(&self, token_sha256: [u8; 32]) -> Result<()> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached("UPDATE sessions SET last_seen_at = ?2 WHERE token_sha256 = ?1")?
                .execute(params![token_sha256, now])?;
            Ok(())
        })
        .await
    }

    async fn delete_session(&self, token_sha256: [u8; 32]) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM sessions WHERE token_sha256 = ?1")?
                .execute([token_sha256])?
                > 0)
        })
        .await
    }

    async fn delete_stale_sessions(&self, idle_since: Timestamp) -> Result<usize> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM sessions WHERE expires_at <= ?1 OR last_seen_at < ?2")?
                .execute(params![now, idle_since.as_unix_ms()])?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::UserId;
    use uniwrt_model::user::Role;

    use uniwrt_core::Clock as _;

    use crate::{Error, NewSession, SessionRepo as _, UserRepo as _, testing};

    fn new(
        token: u8,
        user: UserId,
        expires_in: Duration,
        now: uniwrt_core::Timestamp,
    ) -> NewSession {
        NewSession {
            token_sha256: [token; 32],
            user,
            expires_at: now.saturating_add(expires_in),
            ip: Some("192.168.1.20".into()),
            user_agent: None,
        }
    }

    #[tokio::test]
    async fn sessions_are_created_touched_and_ended() {
        let (_dir, store, clock) = testing::open().await;
        let user = store
            .create_first_user("admin", Role::Admin, "x")
            .await
            .unwrap();
        let start = testing::START;
        let day = Duration::from_hours(24);
        store
            .create_session(new(1, user.id, 30 * day, start))
            .await
            .unwrap();
        let session = store.session([1; 32]).await.unwrap().unwrap();
        assert_eq!(session.user, user);
        assert_eq!((session.created_at, session.last_seen_at), (start, start));
        assert_eq!(session.expires_at, start.saturating_add(30 * day));
        assert_eq!(store.session([2; 32]).await.unwrap(), None);

        clock.advance(Duration::from_secs(60));
        store.touch_session([1; 32]).await.unwrap();
        let session = store.session([1; 32]).await.unwrap().unwrap();
        assert_eq!(
            session.last_seen_at,
            start.saturating_add(Duration::from_secs(60))
        );

        assert!(store.delete_session([1; 32]).await.unwrap());
        assert!(!store.delete_session([1; 32]).await.unwrap());
        assert!(matches!(
            store
                .create_session(new(3, UserId::new(99), day, start))
                .await,
            Err(Error::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn stale_sessions_are_deleted() {
        let (_dir, store, clock) = testing::open().await;
        let user = store
            .create_first_user("admin", Role::Admin, "x")
            .await
            .unwrap();
        let (start, hour) = (testing::START, Duration::from_hours(1));
        store
            .create_session(new(1, user.id, 2 * hour, start))
            .await
            .unwrap(); // expires
        store
            .create_session(new(2, user.id, 99 * hour, start))
            .await
            .unwrap(); // goes idle
        clock.advance(3 * hour);
        store
            .create_session(new(3, user.id, 99 * hour, clock.now()))
            .await
            .unwrap();
        let idle_since = clock.now().saturating_sub(hour);
        assert_eq!(store.delete_stale_sessions(idle_since).await.unwrap(), 2);
        assert!(store.session([3; 32]).await.unwrap().is_some());
    }
}

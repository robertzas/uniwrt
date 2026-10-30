//! [`UserRepo`] on SQLite: the `users` table.

use rusqlite::{OptionalExtension as _, Row, params};
use uniwrt_core::UserId;
use uniwrt_model::user::{Role, User};

use super::UserRepo;
use crate::{
    Error, Result, Store,
    codec::{constraint, enum_column, enum_text, timestamp_column},
};

pub(super) const COLUMNS: &str = "id, username, role, created_at, updated_at";

/// A user from [`COLUMNS`], starting at column `at`.
pub(super) fn user_from_row(row: &Row<'_>, at: usize) -> rusqlite::Result<User> {
    Ok(User {
        id: UserId::new(row.get(at)?),
        username: row.get(at + 1)?,
        role: enum_column(row, at + 2)?,
        created_at: timestamp_column(row, at + 3)?,
        updated_at: timestamp_column(row, at + 4)?,
    })
}

impl UserRepo for Store {
    async fn create_first_user(
        &self,
        username: &str,
        role: Role,
        password_hash: &str,
    ) -> Result<User> {
        let (username, password_hash) = (username.to_owned(), password_hash.to_owned());
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            // One statement, so a concurrent setup sees this user or inserts nothing.
            conn.prepare_cached(&format!(
                "INSERT INTO users (username, password, role, created_at, updated_at)
                 SELECT ?1, ?2, ?3, ?4, ?4 WHERE NOT EXISTS (SELECT 1 FROM users)
                 RETURNING {COLUMNS}"
            ))?
            .query_row(
                params![username, password_hash, enum_text(role), now],
                |row| user_from_row(row, 0),
            )
            .optional()
            .map_err(|err| constraint(err, "user", &username))?
            .ok_or(Error::Conflict {
                what: "the first user",
                key: username,
            })
        })
        .await
    }

    async fn has_users(&self) -> Result<bool> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached("SELECT EXISTS (SELECT 1 FROM users)")?
                .query_row([], |row| row.get(0))?)
        })
        .await
    }

    async fn user(&self, id: UserId) -> Result<Option<User>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!("SELECT {COLUMNS} FROM users WHERE id = ?1"))?
                .query_row([id.get()], |row| user_from_row(row, 0))
                .optional()?)
        })
        .await
    }

    async fn credentials(&self, username: &str) -> Result<Option<(User, String)>> {
        let username = username.to_owned();
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS}, password FROM users WHERE username = ?1"
                ))?
                .query_row([username], |row| Ok((user_from_row(row, 0)?, row.get(5)?)))
                .optional()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::user::Role;

    use crate::{Error, UserRepo as _, testing};

    #[tokio::test]
    async fn only_the_first_user_is_created_and_credentials_find_them() {
        let (_dir, store, _) = testing::open().await;
        assert!(!store.has_users().await.unwrap());
        let admin = store
            .create_first_user("admin", Role::Admin, "$argon2id$hash")
            .await
            .unwrap();
        assert_eq!(
            (admin.username.as_str(), admin.role),
            ("admin", Role::Admin)
        );
        assert!(store.has_users().await.unwrap());
        assert!(matches!(
            store.create_first_user("other", Role::Admin, "x").await,
            Err(Error::Conflict { .. })
        ));

        let (user, hash) = store.credentials("admin").await.unwrap().unwrap();
        assert_eq!((user, hash.as_str()), (admin.clone(), "$argon2id$hash"));
        assert_eq!(store.credentials("other").await.unwrap(), None);
        assert_eq!(store.user(admin.id).await.unwrap(), Some(admin));
    }
}

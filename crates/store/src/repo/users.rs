//! [`UserRepo`] on SQLite: the `users` table.

use rusqlite::{Connection, OptionalExtension as _, Row, params};
use uniwrt_core::{Timestamp, UserId};
use uniwrt_model::user::{Role, User};

use super::{UserActivity, UserChange, UserRepo};
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

    async fn users(&self) -> Result<Vec<UserActivity>> {
        self.read(|conn| {
            let mut statement = conn.prepare_cached(
                "SELECT u.id, u.username, u.role, u.created_at, u.updated_at,
                        (SELECT MAX(s.last_seen_at) FROM sessions s WHERE s.user_id = u.id)
                 FROM users u ORDER BY u.username",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(UserActivity {
                    user: user_from_row(row, 0)?,
                    last_seen_at: row.get::<_, Option<i64>>(5)?.map(Timestamp::from_unix_ms),
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn create_user(&self, username: &str, role: Role, password_hash: &str) -> Result<User> {
        let (username, password_hash) = (username.to_owned(), password_hash.to_owned());
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(&format!(
                "INSERT INTO users (username, password, role, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4) RETURNING {COLUMNS}"
            ))?
            .query_row(
                params![username, password_hash, enum_text(role), now],
                |row| user_from_row(row, 0),
            )
            .map_err(|err| constraint(err, "user", &username))
        })
        .await
    }

    async fn update_user(
        &self,
        id: UserId,
        change: UserChange,
        keep: Option<[u8; 32]>,
    ) -> Result<User> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let user = existing(conn, id)?;
            let role = change.role.filter(|role| *role != user.role);
            if role.is_none() && change.password_hash.is_none() {
                return Ok(user);
            }
            if role == Some(Role::Viewer) {
                keep_an_admin(conn, &user)?;
            }
            let user = conn
                .prepare_cached(&format!(
                    "UPDATE users SET role = COALESCE(?2, role), password = COALESCE(?3, password),
                     updated_at = ?4 WHERE id = ?1 RETURNING {COLUMNS}"
                ))?
                .query_row(
                    params![id.get(), role.map(enum_text), change.password_hash, now],
                    |row| user_from_row(row, 0),
                )?;
            conn.prepare_cached(
                "DELETE FROM sessions WHERE user_id = ?1 AND token_sha256 IS NOT ?2",
            )?
            .execute(params![id.get(), keep.as_ref().map(<[u8; 32]>::as_slice)])?;
            Ok(user)
        })
        .await
    }

    async fn delete_user(&self, id: UserId) -> Result<User> {
        self.write(move |conn| {
            let user = existing(conn, id)?;
            if user.role == Role::Admin {
                keep_an_admin(conn, &user)?;
            }
            // Their sessions go with them (ON DELETE CASCADE); the audit log keeps their name.
            conn.prepare_cached("DELETE FROM users WHERE id = ?1")?
                .execute([id.get()])?;
            Ok(user)
        })
        .await
    }
}

/// The user `id`, or [`Error::NotFound`].
fn existing(conn: &Connection, id: UserId) -> Result<User> {
    conn.prepare_cached(&format!("SELECT {COLUMNS} FROM users WHERE id = ?1"))?
        .query_row([id.get()], |row| user_from_row(row, 0))
        .optional()?
        .ok_or(Error::NotFound {
            what: "user",
            key: id.get().to_string(),
        })
}

/// [`Error::LastAdmin`] unless an admin other than `user` exists.
fn keep_an_admin(conn: &Connection, user: &User) -> Result<()> {
    let others: bool = conn
        .prepare_cached("SELECT EXISTS (SELECT 1 FROM users WHERE role = ?1 AND id != ?2)")?
        .query_row(params![enum_text(Role::Admin), user.id.get()], |row| {
            row.get(0)
        })?;
    if others {
        Ok(())
    } else {
        Err(Error::LastAdmin {
            username: user.username.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::{Timestamp, UserId};
    use uniwrt_model::user::Role;

    use crate::{Error, NewSession, SessionRepo as _, UserChange, UserRepo as _, testing};

    fn session(byte: u8, user: UserId) -> NewSession {
        NewSession {
            token_sha256: [byte; 32],
            user,
            expires_at: Timestamp::from_unix_secs(4_000_000_000),
            ip: None,
            user_agent: None,
        }
    }

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

    #[tokio::test]
    async fn users_are_added_listed_changed_and_deleted_keeping_an_admin() {
        let (_dir, store, clock) = testing::open().await;
        let admin = store
            .create_first_user("admin", Role::Admin, "h0")
            .await
            .unwrap();
        let bob = store.create_user("bob", Role::Viewer, "h1").await.unwrap();
        assert!(matches!(
            store.create_user("bob", Role::Admin, "h2").await,
            Err(Error::Conflict { .. })
        ));
        store.create_session(session(1, admin.id)).await.unwrap();
        store.create_session(session(2, bob.id)).await.unwrap();
        store.create_session(session(3, bob.id)).await.unwrap();

        let listed = store.users().await.unwrap();
        let names: Vec<_> = listed.iter().map(|u| u.user.username.as_str()).collect();
        assert_eq!(names, ["admin", "bob"]);
        assert!(listed.iter().all(|u| u.last_seen_at.is_some()));

        // The last admin can't become a viewer or be deleted.
        let change = |role| UserChange {
            role: Some(role),
            password_hash: None,
        };
        assert!(matches!(
            store.update_user(admin.id, change(Role::Viewer), None).await,
            Err(Error::LastAdmin { username }) if username == "admin"
        ));
        assert!(matches!(
            store.delete_user(admin.id).await,
            Err(Error::LastAdmin { .. })
        ));

        // No change ends no session; a change ends all but the one kept.
        let same = store
            .update_user(bob.id, change(Role::Viewer), None)
            .await
            .unwrap();
        assert_eq!(same, bob);
        assert!(store.session([2; 32]).await.unwrap().is_some());
        clock.advance(Duration::from_secs(1));
        let promoted = store
            .update_user(bob.id, change(Role::Admin), Some([3; 32]))
            .await
            .unwrap();
        assert_eq!(promoted.role, Role::Admin);
        assert!(promoted.updated_at > bob.updated_at);
        assert!(store.session([2; 32]).await.unwrap().is_none());
        assert!(store.session([3; 32]).await.unwrap().is_some());
        let reset = UserChange {
            role: None,
            password_hash: Some("h3".into()),
        };
        store.update_user(bob.id, reset, None).await.unwrap();
        assert_eq!(store.credentials("bob").await.unwrap().unwrap().1, "h3");
        assert!(store.session([3; 32]).await.unwrap().is_none());
        assert!(
            store.session([1; 32]).await.unwrap().is_some(),
            "others' kept"
        );

        // With two admins, either may go.
        store
            .update_user(admin.id, change(Role::Viewer), None)
            .await
            .unwrap();
        assert_eq!(store.delete_user(admin.id).await.unwrap().username, "admin");
        assert!(store.session([1; 32]).await.unwrap().is_none());
        assert!(matches!(
            store.delete_user(admin.id).await,
            Err(Error::NotFound { .. })
        ));
        assert!(matches!(
            store
                .update_user(UserId::new(99), UserChange::default(), None)
                .await,
            Err(Error::NotFound { .. })
        ));
    }
}

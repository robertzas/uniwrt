//! [`VaultRepo`] on SQLite: the `vault` table. A stub until M1.6, which seals every value.

use rusqlite::{OptionalExtension as _, params};
use zeroize::Zeroizing;

use super::{Secret, VaultRepo};
use crate::{Result, Store};

/// How the stub stores a value: unencrypted. M1.6 reseals these rows when the vault opens.
const PLAIN: &str = "plain";

impl VaultRepo for Store {
    async fn put_secret(&self, name: &str, value: Secret) -> Result<()> {
        let name = name.to_owned();
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO vault (name, alg, value, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT (name) DO UPDATE
                 SET alg = excluded.alg, value = excluded.value, updated_at = excluded.updated_at",
            )?
            .execute(params![name, PLAIN, value.as_slice(), now])?;
            Ok(())
        })
        .await
    }

    async fn secret(&self, name: &str) -> Result<Option<Secret>> {
        let name = name.to_owned();
        self.read(move |conn| {
            Ok(conn
                .prepare_cached("SELECT value FROM vault WHERE name = ?1")?
                .query_row([name], |row| row.get::<_, Vec<u8>>(0).map(Zeroizing::new))
                .optional()?)
        })
        .await
    }

    async fn delete_secret(&self, name: &str) -> Result<bool> {
        let name = name.to_owned();
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM vault WHERE name = ?1")?
                .execute([name])?
                > 0)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zeroize::Zeroizing;

    use crate::{VaultRepo as _, testing};

    #[tokio::test]
    async fn secrets_are_stored_replaced_and_deleted_by_name() {
        let (_dir, store, clock) = testing::open().await;
        assert_eq!(store.secret("controller/ssh_key").await.unwrap(), None);

        store
            .put_secret("controller/ssh_key", Zeroizing::new(b"one".to_vec()))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(1));
        store
            .put_secret("controller/ssh_key", Zeroizing::new(b"two".to_vec()))
            .await
            .unwrap();
        store
            .put_secret("other", Zeroizing::new(vec![0, 255]))
            .await
            .unwrap();
        assert_eq!(
            store.secret("controller/ssh_key").await.unwrap().as_deref(),
            Some(&b"two".to_vec())
        );

        let (alg, created, updated): (String, i64, i64) = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT alg, created_at, updated_at FROM vault WHERE name = 'controller/ssh_key'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(alg, "plain");
        assert_eq!(updated - created, 1_000);

        assert!(store.delete_secret("controller/ssh_key").await.unwrap());
        assert!(!store.delete_secret("controller/ssh_key").await.unwrap());
        assert_eq!(store.secret("controller/ssh_key").await.unwrap(), None);
        assert_eq!(
            store.secret("other").await.unwrap().as_deref(),
            Some(&vec![0, 255])
        );
    }
}

//! The vault over a real database: sealing, resealing the stub's rows and refusing a wrong key.

use std::time::Duration;

use rusqlite::params;
use zeroize::Zeroizing;

use super::{ALG, PLAIN, Vault};
use crate::{Error, MasterKey, Store, VaultRepo as _, testing};

/// The rows as stored: name, alg and value.
async fn rows(store: &Store) -> Vec<(String, String, Vec<u8>)> {
    store
        .read(|conn| {
            Ok(conn
                .prepare("SELECT name, alg, value FROM vault ORDER BY name")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
        .unwrap()
}

/// Stores a row as the M1.2 stub did.
async fn put_plain(store: &Store, name: &'static str, value: &'static [u8]) {
    store
        .write(move |conn| {
            conn.execute(
                "INSERT INTO vault (name, alg, value, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 0, 0)",
                params![name, PLAIN, value],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn secrets_are_sealed_replaced_and_deleted_by_name() {
    let (_dir, store, clock) = testing::open().await;
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    assert!(!store.has_sealed_secrets().await.unwrap());
    assert_eq!(vault.secret("controller/ssh_key").await.unwrap(), None);

    vault
        .put_secret("controller/ssh_key", Zeroizing::new(b"one".to_vec()))
        .await
        .unwrap();
    clock.advance(Duration::from_secs(1));
    vault
        .put_secret("controller/ssh_key", Zeroizing::new(b"two".to_vec()))
        .await
        .unwrap();
    vault
        .put_secret("other", Zeroizing::new(vec![0, 255]))
        .await
        .unwrap();
    assert!(store.has_sealed_secrets().await.unwrap());
    assert_eq!(
        vault.secret("controller/ssh_key").await.unwrap().as_deref(),
        Some(&b"two".to_vec())
    );

    for (name, alg, value) in rows(&store).await {
        assert_eq!(alg, ALG, "{name}");
        assert!(
            !value.windows(3).any(|w| w == b"two"),
            "{name} is not sealed"
        );
    }
    let (created, updated): (i64, i64) = store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT created_at, updated_at FROM vault WHERE name = 'controller/ssh_key'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(updated - created, 1_000);

    assert!(vault.delete_secret("controller/ssh_key").await.unwrap());
    assert!(!vault.delete_secret("controller/ssh_key").await.unwrap());
    assert_eq!(vault.secret("controller/ssh_key").await.unwrap(), None);
    assert_eq!(
        vault.secret("other").await.unwrap().as_deref(),
        Some(&vec![0, 255])
    );
}

#[tokio::test]
async fn opening_reseals_the_stubs_rows() {
    let (_dir, store, _) = testing::open().await;
    put_plain(&store, "controller/ssh_key", b"pem").await;
    put_plain(&store, "empty", b"").await;
    assert!(!store.has_sealed_secrets().await.unwrap());

    let key = MasterKey::generate().unwrap();
    let vault = Vault::open(&store, &key).await.unwrap();
    assert!(rows(&store).await.iter().all(|(_, alg, _)| alg == ALG));
    assert_eq!(
        vault.secret("controller/ssh_key").await.unwrap().as_deref(),
        Some(&b"pem".to_vec())
    );
    assert_eq!(
        vault.secret("empty").await.unwrap().as_deref(),
        Some(&Vec::new())
    );

    // Opening again finds nothing to reseal and leaves the rows as they are.
    let before = rows(&store).await;
    Vault::open(&store, &key).await.unwrap();
    assert_eq!(rows(&store).await, before);
}

#[tokio::test]
async fn a_wrong_master_key_is_refused_and_changes_nothing() {
    let (_dir, store, _) = testing::open().await;
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    vault
        .put_secret("sealed", Zeroizing::new(b"x".to_vec()))
        .await
        .unwrap();
    put_plain(&store, "plain", b"y").await;
    let before = rows(&store).await;

    let err = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::WrongMasterKey {
                unreadable: 1,
                total: 2
            }
        ),
        "{err:?}"
    );
    assert_eq!(rows(&store).await, before);
}

#[tokio::test]
async fn altered_and_unknown_rows_are_errors() {
    let (_dir, store, _) = testing::open().await;
    let vault = Vault::open(&store, &MasterKey::generate().unwrap())
        .await
        .unwrap();
    vault
        .put_secret("a", Zeroizing::new(b"x".to_vec()))
        .await
        .unwrap();
    vault
        .put_secret("b", Zeroizing::new(b"y".to_vec()))
        .await
        .unwrap();
    // A sealed value moved to another name does not open there.
    store
        .write(|conn| {
            conn.execute(
                "UPDATE vault SET value = (SELECT value FROM vault WHERE name = 'a') WHERE name = 'b'",
                [],
            )?;
            conn.execute(
                "INSERT INTO vault (name, alg, value, created_at, updated_at)
                 VALUES ('c', 'rot13', x'00', 0, 0)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        vault.secret("b").await,
        Err(Error::Unseal { name }) if name == "b"
    ));
    assert!(matches!(
        vault.secret("c").await,
        Err(Error::UnknownSeal { name, alg }) if name == "c" && alg == "rot13"
    ));
    assert_eq!(
        vault.secret("a").await.unwrap().as_deref(),
        Some(&b"x".to_vec())
    );
}

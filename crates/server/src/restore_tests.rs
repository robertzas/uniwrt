use std::{fs, sync::Arc};

use uniwrt_core::SystemClock;
use uniwrt_store::{
    MasterKey, Store, Vault, VaultRepo as _,
    controller_backup::{ControllerBackups, Staging},
};
use zeroize::Zeroizing;

use super::{KeyFile, apply_staged};
use crate::master_key;

const PASS: &[u8] = b"a long passphrase";

/// A backup of a database holding secret `a`, sealed under the key returned.
async fn backup(dir: &std::path::Path) -> (std::path::PathBuf, MasterKey) {
    let store = Store::open(dir.join("old.db"), Arc::new(SystemClock))
        .await
        .unwrap();
    let key = MasterKey::generate().unwrap();
    let vault = Vault::open(&store, &key).await.unwrap();
    vault
        .put_secret("a", Zeroizing::new(b"one".to_vec()))
        .await
        .unwrap();
    let backups = ControllerBackups::new(dir.join("backups"), None).with_work_factor(4);
    let taken = backups.take(&store, &key, PASS, "1").await.unwrap();
    store.close().await;
    (backups.file(&taken.backup.name).unwrap(), key)
}

async fn secret(data: &std::path::Path, key: &MasterKey) -> Option<Vec<u8>> {
    let store = Store::open(data.join("uniwrt.db"), Arc::new(SystemClock))
        .await
        .unwrap();
    let value = Vault::open(&store, key)
        .await
        .unwrap()
        .secret("a")
        .await
        .unwrap();
    store.close().await;
    value.map(|v| v.to_vec())
}

#[tokio::test]
async fn a_staged_restore_replaces_the_database_and_key_and_keeps_them_aside() {
    let src = tempfile::tempdir().unwrap();
    let (file, key) = backup(src.path()).await;
    let data = tempfile::tempdir().unwrap();
    // The host's own database and key, to be replaced.
    fs::write(data.path().join("uniwrt.db"), "old").unwrap();
    fs::write(data.path().join("uniwrt.db-wal"), "old wal").unwrap();
    let key_path = data.path().join("master.key");
    master_key::write(&key_path, &MasterKey::generate().unwrap()).unwrap();
    let old_key = fs::read_to_string(&key_path).unwrap();
    let key_file = KeyFile {
        path: &key_path,
        named: false,
    };

    // Nothing staged: nothing happens.
    apply_staged(data.path(), &key_file).await.unwrap();
    assert_eq!(
        fs::read_to_string(data.path().join("uniwrt.db")).unwrap(),
        "old"
    );

    Staging::new(data.path()).stage(file, PASS).await.unwrap();
    apply_staged(data.path(), &key_file).await.unwrap();
    assert_eq!(
        fs::read_to_string(&key_path).unwrap().trim(),
        key.to_text().as_str()
    );
    assert_eq!(
        secret(data.path(), &key).await.as_deref(),
        Some(&b"one"[..])
    );
    let previous = data.path().join("restore/previous");
    assert_eq!(
        fs::read_to_string(previous.join("uniwrt.db")).unwrap(),
        "old"
    );
    assert_eq!(
        fs::read_to_string(previous.join("uniwrt.db-wal")).unwrap(),
        "old wal"
    );
    assert_eq!(
        fs::read_to_string(previous.join("master.key")).unwrap(),
        old_key
    );
    assert!(!data.path().join("restore/ready").exists());
    // Applied once only.
    apply_staged(data.path(), &key_file).await.unwrap();
    assert_eq!(
        secret(data.path(), &key).await.as_deref(),
        Some(&b"one"[..])
    );
}

#[tokio::test]
async fn a_host_that_names_its_own_key_keeps_it_and_the_vault_is_resealed() {
    let src = tempfile::tempdir().unwrap();
    let (file, _) = backup(src.path()).await;
    let data = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let key_path = secrets.path().join("master.key");
    let host = MasterKey::generate().unwrap();
    master_key::write(&key_path, &host).unwrap();
    let text = fs::read_to_string(&key_path).unwrap();

    Staging::new(data.path()).stage(file, PASS).await.unwrap();
    apply_staged(
        data.path(),
        &KeyFile {
            path: &key_path,
            named: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(fs::read_to_string(&key_path).unwrap(), text);
    assert_eq!(
        secret(data.path(), &host).await.as_deref(),
        Some(&b"one"[..])
    );
    assert!(!data.path().join("restore/previous/master.key").exists());
}

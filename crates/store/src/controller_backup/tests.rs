use std::{fs, os::unix::fs::PermissionsExt as _, sync::Arc, time::Duration};

use uniwrt_core::SystemClock;
use uniwrt_model::controller_backup::{BackupManifest, KEEP, RestoreState};
use zeroize::Zeroizing;

use super::{
    ControllerBackups, DATABASE, MASTER_KEY, Staging, stage::unpack_within, unpack, write_backup,
};
use crate::{Error, MasterKey, Store, Vault, VaultRepo as _, testing};

const PASS: &[u8] = b"a long passphrase";

async fn vault_with_a_secret(store: &Store) -> (MasterKey, Vault) {
    let key = MasterKey::generate().unwrap();
    let vault = Vault::open(store, &key).await.unwrap();
    vault
        .put_secret("a", Zeroizing::new(b"one".to_vec()))
        .await
        .unwrap();
    (key, vault)
}

#[tokio::test]
async fn a_backup_holds_the_database_and_key_and_unpacks_whole() {
    let (dir, store, _) = testing::open().await;
    let (key, _vault) = vault_with_a_secret(&store).await;
    let backups = ControllerBackups::new(dir.path().join("backups"), Some(dir.path().join("nas")))
        .with_work_factor(4);
    let taken = backups.take(&store, &key, PASS, "1.2.3").await.unwrap();
    assert_eq!(taken.copy_error, None);
    assert!(taken.backup.copied && taken.backup.size_bytes > 0);
    assert_eq!(
        taken.backup.name,
        "uniwrt-backup-2026-10-27T000000Z.tar.gz.age"
    );
    assert_eq!(
        backups.list().await.unwrap(),
        std::slice::from_ref(&taken.backup)
    );
    let file = backups.file(&taken.backup.name).unwrap();
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(dir.path().join("nas").join(&taken.backup.name).is_file());
    // Nothing else is left behind: no snapshot, no partial file.
    assert_eq!(fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    assert_eq!(backups.file("../test.db"), None);
    assert!(!backups.running());

    let out = tempfile::tempdir().unwrap();
    let manifest = unpack(fs::File::open(&file).unwrap(), PASS, out.path()).unwrap();
    assert_eq!(
        (manifest.version.as_str(), manifest.taken_at),
        ("1.2.3", testing::START)
    );
    assert_eq!(
        manifest.schema,
        i64::try_from(crate::migrate::latest()).unwrap()
    );
    let text = fs::read_to_string(out.path().join(MASTER_KEY)).unwrap();
    assert_eq!(text.trim(), key.to_text().as_str());
    let restored = Store::open(out.path().join(DATABASE), Arc::new(SystemClock))
        .await
        .unwrap();
    let vault = Vault::open(&restored, &MasterKey::parse(&text).unwrap())
        .await
        .unwrap();
    assert_eq!(
        vault.secret("a").await.unwrap().as_deref(),
        Some(&b"one".to_vec())
    );

    assert!(matches!(
        unpack(fs::File::open(&file).unwrap(), b"wrong", out.path()),
        Err(Error::Passphrase)
    ));
    let mut cut = fs::read(&file).unwrap();
    cut.truncate(cut.len() - 20);
    assert!(unpack(cut.as_slice(), PASS, out.path()).is_err());
}

#[tokio::test]
async fn the_newest_seven_are_kept_in_each_directory() {
    let (dir, store, clock) = testing::open().await;
    let (key, _vault) = vault_with_a_secret(&store).await;
    let backups = ControllerBackups::new(dir.path().join("b"), Some(dir.path().join("c")))
        .with_work_factor(4);
    // Something else in the directory is left alone.
    fs::create_dir_all(dir.path().join("b")).unwrap();
    fs::write(dir.path().join("b/notes.txt"), "mine").unwrap();
    for _ in 0..KEEP + 2 {
        backups.take(&store, &key, PASS, "1").await.unwrap();
        clock.advance(Duration::from_hours(24));
    }
    let list = backups.list().await.unwrap();
    assert_eq!(list.len(), KEEP);
    assert!(list.windows(2).all(|w| w[0].taken_at > w[1].taken_at));
    assert_eq!(
        list[0].taken_at,
        testing::START.saturating_add(Duration::from_hours(24 * 8))
    );
    assert_eq!(fs::read_dir(dir.path().join("c")).unwrap().count(), KEEP);
    assert!(dir.path().join("b/notes.txt").is_file());
}

#[tokio::test]
async fn a_restore_is_staged_from_an_upload_once_its_passphrase_opens_it() {
    let (dir, store, _) = testing::open().await;
    let (key, _vault) = vault_with_a_secret(&store).await;
    let backups = ControllerBackups::new(dir.path().join("backups"), None).with_work_factor(4);
    let taken = backups.take(&store, &key, PASS, "1").await.unwrap();
    assert!(!taken.backup.copied);

    let data = tempfile::tempdir().unwrap();
    let staging = Staging::new(data.path());
    assert_eq!(staging.state().await, RestoreState::None);
    assert!(staging.ready().unwrap().is_none());
    let upload = staging.upload_path().unwrap();
    fs::copy(backups.file(&taken.backup.name).unwrap(), &upload).unwrap();
    assert!(
        matches!(staging.state().await, RestoreState::Uploaded { size_bytes } if size_bytes == taken.backup.size_bytes)
    );
    assert!(matches!(
        staging.unlock(b"wrong").await,
        Err(Error::Passphrase)
    ));
    assert!(matches!(
        staging.state().await,
        RestoreState::Uploaded { .. }
    ));

    let manifest = staging.unlock(PASS).await.unwrap();
    assert_eq!(
        staging.state().await,
        RestoreState::Ready {
            manifest: manifest.clone()
        }
    );
    assert!(!upload.exists());
    let staged = staging.ready().unwrap().unwrap();
    assert_eq!(staged.manifest, manifest);
    assert_eq!(staged.key.to_text(), key.to_text());
    assert!(staged.dir.join(DATABASE).is_file());

    staging.discard().await;
    assert_eq!(staging.state().await, RestoreState::None);
}

#[tokio::test]
async fn a_backup_of_a_newer_schema_or_a_mismatched_key_is_refused() {
    let (dir, store, _) = testing::open().await;
    let (key, _vault) = vault_with_a_secret(&store).await;
    let snapshot = dir.path().join("snap.db");
    fs::File::create(&snapshot).unwrap();
    store.snapshot_into(snapshot.clone()).await.unwrap();
    let write = |name: &str, schema: i64, key: &MasterKey| {
        let manifest = BackupManifest {
            format: 1,
            version: "9".into(),
            schema,
            taken_at: testing::START,
        };
        let target = dir.path().join(name);
        write_backup(&target, &snapshot, &manifest, &key.to_text(), PASS, 4).unwrap();
        target
    };
    let newer = write("newer", 999, &key);
    let out = tempfile::tempdir().unwrap();
    assert!(matches!(
        unpack(fs::File::open(newer).unwrap(), PASS, out.path()),
        Err(Error::TooNew { found: 999, .. })
    ));

    let other = write("other", 1, &MasterKey::generate().unwrap());
    let staging = Staging::new(out.path());
    assert!(matches!(
        staging.stage(other, PASS).await,
        Err(Error::WrongMasterKey { .. })
    ));
    assert_eq!(staging.state().await, RestoreState::None);
}

#[tokio::test]
async fn rekeying_reseals_every_secret_and_node_backup() {
    let (_dir, store, _) = testing::open().await;
    let (_, vault) = vault_with_a_secret(&store).await;
    let new = MasterKey::generate().unwrap();
    let vault = vault.rekey(&new).await.unwrap();
    assert_eq!(
        vault.secret("a").await.unwrap().as_deref(),
        Some(&b"one".to_vec())
    );
    let reopened = Vault::open(&store, &new).await.unwrap();
    assert_eq!(
        reopened.secret("a").await.unwrap().as_deref(),
        Some(&b"one".to_vec())
    );
    assert!(matches!(
        Vault::open(&store, &MasterKey::generate().unwrap()).await,
        Err(Error::WrongMasterKey { .. })
    ));
}

#[tokio::test]
async fn a_database_that_unpacks_far_past_the_backup_s_size_is_refused() {
    let (dir, store, _) = testing::open().await;
    let (key, _vault) = vault_with_a_secret(&store).await;
    // 16 MiB of zeros: gzip packs them into a few KiB.
    let snapshot = dir.path().join("zeros.db");
    fs::File::create(&snapshot)
        .unwrap()
        .set_len(16 << 20)
        .unwrap();
    let manifest = BackupManifest {
        format: 1,
        version: "9".into(),
        schema: 1,
        taken_at: testing::START,
    };
    let bomb = dir.path().join("bomb");
    write_backup(&bomb, &snapshot, &manifest, &key.to_text(), PASS, 4).unwrap();
    assert!(fs::metadata(&bomb).unwrap().len() < 256 << 10);
    let out = tempfile::tempdir().unwrap();
    let refused = unpack_within(fs::File::open(&bomb).unwrap(), PASS, out.path(), 1 << 20);
    assert!(
        matches!(&refused, Err(Error::NotBackup(why)) if why.contains("far more")),
        "{refused:?}"
    );
    drop(store);
    // The same file within the real allowance unpacks.
    unpack(fs::File::open(&bomb).unwrap(), PASS, out.path()).unwrap();
}

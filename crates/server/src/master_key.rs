//! The vault's master key (features/security.md, Secrets vault): from `UNIWRT_MASTER_KEY`, or
//! from its file (`<data dir>/master.key` unless `--master-key-file` says otherwise), which the
//! first run creates with mode 0600.
//!
//! A missing file is replaced by a new key only while the vault holds nothing sealed: a new key
//! would not open the old secrets, so then startup fails and asks for the lost file instead.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write as _},
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::Path,
};

use anyhow::{Context as _, bail};
use tracing::{info, warn};
use uniwrt_store::{MasterKey, Store, Vault};

/// The environment variable that holds the key itself, which then never touches the disk.
const ENV: &str = "UNIWRT_MASTER_KEY";

/// Where the key comes from.
pub(crate) enum Source<'a> {
    /// `UNIWRT_MASTER_KEY` if set, else the file at this path.
    Configured(&'a Path),
    /// A fresh key held only in memory, for demo mode's throwaway database.
    Ephemeral,
}

/// Loads the master key, creating its file on the first run, and opens the vault with it.
pub(crate) async fn open_vault(store: &Store, source: Source<'_>) -> anyhow::Result<Vault> {
    let key = match source {
        Source::Ephemeral => MasterKey::generate()?,
        Source::Configured(path) => match std::env::var(ENV) {
            Ok(text) => MasterKey::parse(&text).with_context(|| format!("{ENV} is malformed"))?,
            Err(std::env::VarError::NotUnicode(_)) => bail!("{ENV} is not valid UTF-8"),
            Err(std::env::VarError::NotPresent) => load_or_create(store, path).await?,
        },
    };
    Vault::open(store, &key)
        .await
        .context("cannot open the vault")
}

async fn load_or_create(store: &Store, path: &Path) -> anyhow::Result<MasterKey> {
    match read(path) {
        Ok(key) => return key,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("cannot read the master key {}", path.display()));
        }
    }
    if store.has_sealed_secrets().await? {
        bail!(
            "the master key {} is missing, but the vault holds secrets sealed under it; \
             restore the file or set {ENV}",
            path.display()
        );
    }
    let key = MasterKey::generate()?;
    create(path, &key)
        .with_context(|| format!("cannot create the master key {}", path.display()))?;
    info!(path = %path.display(), "generated the vault's master key; back it up apart from the database");
    Ok(key)
}

/// Reads the key file; the outer error is the file's, the inner one its content's.
fn read(path: &Path) -> io::Result<anyhow::Result<MasterKey>> {
    let text = zeroize::Zeroizing::new(fs::read_to_string(path)?);
    let mode = fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        warn!(path = %path.display(), mode = format_args!("{:o}", mode & 0o777), "the master key is readable by other users; chmod 600 it");
    }
    Ok(MasterKey::parse(&text)
        .with_context(|| format!("the master key {} is malformed", path.display())))
}

fn create(path: &Path, key: &MasterKey) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let mut text = key.to_text();
    text.push('\n');
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _, sync::Arc};

    use uniwrt_core::SystemClock;
    use uniwrt_store::{Store, VaultRepo as _};
    use zeroize::Zeroizing;

    use super::{Source, load_or_create, open_vault};

    async fn store(dir: &tempfile::TempDir) -> Store {
        Store::open(dir.path().join("uniwrt.db"), Arc::new(SystemClock))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_first_run_creates_a_private_key_file_that_later_runs_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys/master.key");
        let store = store(&dir).await;
        let vault = open_vault(&store, Source::Configured(&path)).await.unwrap();
        vault
            .put_secret("a", Zeroizing::new(b"x".to_vec()))
            .await
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.len(), 65);
        assert!(text.ends_with('\n'));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let vault = open_vault(&store, Source::Configured(&path)).await.unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(
            vault.secret("a").await.unwrap().as_deref(),
            Some(&b"x".to_vec())
        );
        store.close().await;
    }

    #[tokio::test]
    async fn a_lost_key_is_not_replaced_while_secrets_are_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.key");
        let store = store(&dir).await;
        let vault = open_vault(&store, Source::Configured(&path)).await.unwrap();
        vault
            .put_secret("a", Zeroizing::new(b"x".to_vec()))
            .await
            .unwrap();
        fs::remove_file(&path).unwrap();

        let err = load_or_create(&store, &path).await.unwrap_err();
        assert!(err.to_string().contains("is missing"), "{err:#}");
        assert!(!path.exists());
        store.close().await;
    }

    #[tokio::test]
    async fn a_malformed_key_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.key");
        fs::write(&path, "not a key\n").unwrap();
        let store = store(&dir).await;
        let err = load_or_create(&store, &path).await.unwrap_err();
        assert!(err.to_string().contains("is malformed"), "{err:#}");
        store.close().await;
    }
}

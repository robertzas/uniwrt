//! Restoring a controller backup (features/lifecycle.md, Backups; D-129): `uniwrt restore FILE`
//! stages it as an upload in the UI does ([`run`]), and the next start puts it in place before the
//! store opens ([`apply_staged`]).
//!
//! Applying moves the database it replaces (and the key file, when that is replaced too) to
//! `<data dir>/restore/previous`, kept until the next restore. The key: a host that keeps its own
//! (`UNIWRT_MASTER_KEY`, or a `--master-key-file` that exists, such as a Docker secret) keeps it,
//! and the restored vault is resealed under it; otherwise the backup's key becomes the key file.

use std::{
    fs,
    io::{self, BufRead as _, IsTerminal as _, Write as _},
    os::unix::fs::DirBuilderExt as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, bail};
use tracing::info;
use uniwrt_core::SystemClock;
use uniwrt_store::{
    Store, Vault,
    controller_backup::{DATABASE, Staging},
};
use zeroize::Zeroizing;

use crate::{cli::RestoreArgs, master_key};

/// The passphrase's environment variable, for scripts.
const PASSPHRASE_ENV: &str = "UNIWRT_BACKUP_PASSPHRASE";

/// Where the key file is, and whether it was named (`--master-key-file`) rather than defaulted.
pub(crate) struct KeyFile<'a> {
    pub(crate) path: &'a Path,
    pub(crate) named: bool,
}

/// Stages the backup at `args.file` for the next start.
pub(crate) fn run(args: &RestoreArgs) -> anyhow::Result<()> {
    let passphrase = passphrase()?;
    let data_dir = std::path::absolute(&args.data_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    let manifest = runtime
        .block_on(Staging::new(&data_dir).stage(args.file.clone(), passphrase.as_bytes()))
        .with_context(|| format!("cannot restore {}", args.file.display()))?;
    println!(
        "Staged the backup taken at {} by uniwrt {}: the next start of the controller with the data \
         directory {} restores it.",
        manifest.taken_at,
        manifest.version,
        data_dir.display()
    );
    Ok(())
}

/// The passphrase: from [`PASSPHRASE_ENV`], else a line on stdin (not echoed on a terminal).
fn passphrase() -> anyhow::Result<Zeroizing<String>> {
    if let Some(text) = std::env::var_os(PASSPHRASE_ENV) {
        return Ok(Zeroizing::new(text.into_string().map_err(|_| {
            anyhow::anyhow!("{PASSPHRASE_ENV} is not valid UTF-8")
        })?));
    }
    let stdin = io::stdin();
    let terminal = stdin.is_terminal();
    let echo = if terminal {
        eprint!("Passphrase: ");
        io::stderr().flush()?;
        quiet(&stdin)
    } else {
        None
    };
    let mut line = Zeroizing::new(String::new());
    let read = stdin.lock().read_line(&mut line);
    if let Some(termios) = echo {
        let _ = rustix::termios::tcsetattr(&stdin, rustix::termios::OptionalActions::Now, &termios);
        eprintln!();
    }
    read.context("cannot read the passphrase")?;
    let text = Zeroizing::new(line.trim_end_matches(['\n', '\r']).to_owned());
    if text.is_empty() {
        bail!("no passphrase given (type it, pipe it in, or set {PASSPHRASE_ENV})");
    }
    Ok(text)
}

/// Turns the terminal's echo off; returns the settings to put back.
fn quiet(stdin: &io::Stdin) -> Option<rustix::termios::Termios> {
    let saved = rustix::termios::tcgetattr(stdin).ok()?;
    let mut quiet = saved.clone();
    quiet.local_modes.remove(rustix::termios::LocalModes::ECHO);
    rustix::termios::tcsetattr(stdin, rustix::termios::OptionalActions::Now, &quiet).ok()?;
    Some(saved)
}

/// Puts a staged restore in place in `data_dir` (whose database is [`DATABASE`]), before the store
/// opens; nothing when none is staged.
pub(crate) async fn apply_staged(data_dir: &Path, key: &KeyFile<'_>) -> anyhow::Result<()> {
    let staging = Staging::new(data_dir);
    let Some(staged) = staging.ready().context("cannot read the staged restore")? else {
        return Ok(());
    };
    let host_key = master_key::host_key(key.path, key.named)?;
    let database = staged.dir.join(DATABASE);
    // Absent when an interrupted start already moved it in.
    if database.exists()
        && let Some(host) = &host_key
        && host.to_text() != staged.key.to_text()
    {
        let store = Store::open(&database, Arc::new(SystemClock))
            .await
            .context("cannot open the restored database")?;
        let rekeyed = async { Vault::open(&store, &staged.key).await?.rekey(host).await }.await;
        store.close().await;
        rekeyed.context("cannot reseal the restored vault under this host's master key")?;
    }
    let previous = staging.previous_dir();
    let moved = (|| -> io::Result<()> {
        if database.exists() {
            let _ = fs::remove_dir_all(&previous);
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&previous)?;
            for name in [DATABASE, "uniwrt.db-wal", "uniwrt.db-shm"] {
                move_if_there(&data_dir.join(name), &previous.join(name))?;
            }
            fs::rename(&database, data_dir.join(DATABASE))?;
        }
        Ok(())
    })();
    moved.context("cannot put the restored database in place")?;
    if host_key.is_none() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&previous)?;
        move_if_there(key.path, &previous.join("master.key"))
            .context("cannot set the replaced master key aside")?;
        master_key::write(key.path, &staged.key).with_context(|| {
            format!(
                "cannot write the restored master key to {}",
                key.path.display()
            )
        })?;
    }
    fs::remove_dir_all(&staged.dir).context("cannot clear the staged restore")?;
    info!(
        taken_at = %staged.manifest.taken_at,
        version = staged.manifest.version,
        previous = %previous.display(),
        resealed = host_key.is_some(),
        "restored the controller backup; what it replaced is kept aside"
    );
    Ok(())
}

fn move_if_there(from: &Path, to: &PathBuf) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;

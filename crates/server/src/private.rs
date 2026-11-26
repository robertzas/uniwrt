//! The controller's files are its own (threat-model.md §7, D-150): every file and directory
//! it makes is the user's alone (a 077 umask, from the start of every command, kept across
//! the launcher's exec), and serve warns when the data directory lets other users in.

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use rustix::fs::Mode;
use tracing::warn;

/// Takes group and other permissions off every file and directory this process makes from
/// now on: files 0600, directories 0700, the staged binary 0700.
pub(crate) fn restrict_new_files() {
    rustix::process::umask(Mode::from_raw_mode(0o077));
}

/// The data directory, absolute, made if missing; told about when others may open it.
pub(crate) async fn data_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let dir = std::path::absolute(dir)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("cannot create the data directory {}", dir.display()))?;
    warn_if_shared(&dir);
    Ok(dir)
}

/// Warns when other users may open `dir` (it was made before this umask, or by hand);
/// returns whether they may.
pub(crate) fn warn_if_shared(dir: &Path) -> bool {
    let Ok(meta) = fs::metadata(dir) else {
        return false;
    };
    let mode = meta.permissions().mode() & 0o777;
    let shared = mode & 0o077 != 0;
    if shared {
        warn!(
            path = %dir.display(),
            mode = format_args!("{mode:o}"),
            "other users can open the data directory; chmod 700 it"
        );
    }
    shared
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _};

    use super::{restrict_new_files, warn_if_shared};

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn new_files_are_the_user_s_alone_and_a_shared_data_dir_is_told() {
        restrict_new_files();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("f");
        fs::write(&file, "x").unwrap();
        let sub = dir.path().join("d");
        fs::create_dir(&sub).unwrap();
        assert_eq!((mode(&file), mode(&sub)), (0o600, 0o700));

        assert!(!warn_if_shared(&sub));
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(warn_if_shared(&sub));
        assert!(!warn_if_shared(&dir.path().join("missing")));
    }
}

//! The launcher's directory, `<data>/bin` (D-121, D-123): the staged binaries (`uniwrt-X.Y.Z`),
//! `state.json` (the confirmed binary, the one on trial, those rejected and the last failure),
//! and `rollback/`, the database as it was before a trial.

use std::{
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uniwrt_firmware::{Version, binary_name};

const STATE: &str = "state.json";
const ROLLBACK: &str = "rollback";
/// The database's files a trial may change, as the store names them.
pub(super) const DATABASE_FILES: [&str; 3] = ["uniwrt.db", "uniwrt.db-wal", "uniwrt.db-shm"];
const MAX_REJECTED: usize = 8;

/// What the launcher keeps between runs. Versions are `X.Y.Z` strings, so the file reads well.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct State {
    /// The staged binary that passed its trial, run instead of the image's when newer.
    pub(crate) current: Option<String>,
    /// The staged binary to try next (set by the controller before it asks for a restart).
    pub(crate) trial: Option<String>,
    /// The trial the database snapshot in `rollback/` was taken for.
    pub(crate) snapshot: Option<String>,
    /// Binaries that failed their trial: never tried again.
    pub(crate) rejected: Vec<String>,
    /// The last trial that failed, for the update notice.
    pub(crate) failure: Option<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Failure {
    pub(crate) version: String,
    pub(crate) reason: String,
    /// Unix seconds.
    pub(crate) at: i64,
}

/// The binary to run next.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) version: Version,
    /// `None`: the image's own binary.
    pub(crate) staged: Option<PathBuf>,
    pub(crate) trial: bool,
}

/// `<data>/bin` and its files.
#[derive(Clone, Debug)]
pub(crate) struct Bin {
    pub(crate) dir: PathBuf,
    data: PathBuf,
}

impl Bin {
    pub(crate) fn new(data: &Path) -> Self {
        Self {
            dir: data.join("bin"),
            data: data.to_owned(),
        }
    }

    pub(crate) fn binary(&self, version: Version) -> PathBuf {
        self.dir.join(binary_name(version))
    }

    /// The state kept; a missing or unreadable file reads as none (the image's binary runs).
    pub(crate) fn load(&self) -> State {
        match std::fs::read(self.dir.join(STATE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(error = %err, "the launcher's state is unreadable; starting afresh");
                State::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => State::default(),
            Err(err) => {
                tracing::warn!(error = %err, "the launcher's state can't be read; starting afresh");
                State::default()
            }
        }
    }

    /// Writes the state through a temporary file, so a crash leaves the old or the new one.
    pub(crate) fn save(&self, state: &State) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let part = self.dir.join(".state.json.part");
        let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        std::fs::write(&part, bytes)?;
        std::fs::File::open(&part)?.sync_all()?;
        std::fs::rename(&part, self.dir.join(STATE))
    }

    /// What to run: the trial when one is pending, else the confirmed binary, each only while
    /// newer than the image's `builtin` and present; else the image's. Clears what no longer
    /// applies (an image newer than the staged binary wins) and removes binaries nothing names.
    pub(crate) fn choose(&self, state: &mut State, builtin: Version) -> Choice {
        let usable = |name: &Option<String>| -> Option<Version> {
            let version: Version = name.as_deref()?.parse().ok()?;
            (version > builtin
                && !state.rejected.iter().any(|r| r == &version.to_string())
                && self.binary(version).is_file())
            .then_some(version)
        };
        let trial = usable(&state.trial);
        if trial.is_none() {
            state.trial = None;
        }
        let current = usable(&state.current);
        if current.is_none() {
            state.current = None;
        }
        self.prune(state);
        match (trial, current) {
            (Some(version), _) => Choice {
                version,
                staged: Some(self.binary(version)),
                trial: true,
            },
            (None, Some(version)) => Choice {
                version,
                staged: Some(self.binary(version)),
                trial: false,
            },
            (None, None) => Choice {
                version: builtin,
                staged: None,
                trial: false,
            },
        }
    }

    /// Removes staged binaries and leftovers the state doesn't name.
    fn prune(&self, state: &State) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let kept: Vec<String> = [&state.current, &state.trial]
            .into_iter()
            .flatten()
            .filter_map(|v| v.parse().ok().map(binary_name))
            .collect();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let unnamed = (name.starts_with("uniwrt-") && !kept.contains(&name))
                || (name.starts_with('.')
                    && Path::new(&name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("part")));
            if unnamed
                && entry.path().is_file()
                && let Err(err) = std::fs::remove_file(entry.path())
            {
                tracing::warn!(file = %name, error = %err, "cannot remove a stale binary");
            }
        }
    }

    /// Copies the database's files to `rollback/` (the controller is stopped).
    pub(crate) fn snapshot(&self) -> io::Result<()> {
        let rollback = self.dir.join(ROLLBACK);
        let _ = std::fs::remove_dir_all(&rollback);
        std::fs::create_dir_all(&rollback)?;
        for name in DATABASE_FILES {
            let from = self.data.join(name);
            if from.is_file() {
                std::fs::copy(&from, rollback.join(name))?;
            }
        }
        Ok(())
    }

    /// Puts the database back as the snapshot has it: its files copied back, the others removed.
    pub(crate) fn restore(&self) -> io::Result<()> {
        let rollback = self.dir.join(ROLLBACK);
        if !rollback.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no database snapshot to restore",
            ));
        }
        for name in DATABASE_FILES {
            let (kept, live) = (rollback.join(name), self.data.join(name));
            if kept.is_file() {
                std::fs::copy(&kept, &live)?;
            } else if live.exists() {
                std::fs::remove_file(&live)?;
            }
        }
        Ok(())
    }

    pub(crate) fn drop_snapshot(&self) {
        let _ = std::fs::remove_dir_all(self.dir.join(ROLLBACK));
    }
}

impl State {
    /// The trial passed: its binary becomes the current one.
    pub(crate) fn confirm(&mut self, version: Version) {
        self.current = Some(version.to_string());
        self.trial = None;
        self.snapshot = None;
        self.failure = None;
    }

    /// The trial failed: never tried again, and the failure kept for the notice.
    pub(crate) fn reject(&mut self, version: Version, reason: String, at: i64) {
        let version = version.to_string();
        self.trial = None;
        self.snapshot = None;
        self.rejected.retain(|r| r != &version);
        self.rejected.push(version.clone());
        if self.rejected.len() > MAX_REJECTED {
            self.rejected.remove(0);
        }
        self.failure = Some(Failure {
            version,
            reason,
            at,
        });
    }
}

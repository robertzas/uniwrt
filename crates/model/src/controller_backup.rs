//! Controller backups (lifecycle.md, Backups; D-126, D-129): age-encrypted tarballs of the
//! database and the master key, taken nightly once an admin has set the passphrase, the newest
//! [`KEEP`] in each backup directory ([`ControllerBackup`]), each described by its
//! [`BackupManifest`]; and a restore staged from one ([`RestoreState`]), applied at the next start.

use serde::{Deserialize, Serialize};
use uniwrt_core::Timestamp;

/// Backups kept per directory.
pub const KEEP: usize = 7;
/// The nightly window: 02:00 site time for 30 minutes, before the controller's own update
/// (02:30) and the node backups (03:00).
pub const AT_MIN: u16 = 2 * 60;
pub const WINDOW_MIN: u16 = 30;
/// The shortest passphrase accepted.
pub const MIN_PASSPHRASE: usize = 12;
/// The vault's name for the passphrase.
pub const PASSPHRASE_SECRET: &str = "controller/backup_passphrase";

/// A backup file in the backup directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ControllerBackup {
    /// Its file name: `uniwrt-backup-<UTC time>.tar.gz.age`.
    pub name: String,
    pub taken_at: Timestamp,
    pub size_bytes: u64,
    /// Whether the extra backup directory (`--backup-dir`) holds a copy too.
    pub copied: bool,
}

/// `uniwrt-backup.json`, the tarball's first entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BackupManifest {
    /// The tarball's layout: [`BackupManifest::FORMAT`].
    pub format: u32,
    /// The uniwrt version that took it.
    pub version: String,
    /// The database's schema version (`PRAGMA user_version`).
    pub schema: i64,
    pub taken_at: Timestamp,
}

impl BackupManifest {
    pub const FORMAT: u32 = 1;
    pub const NAME: &'static str = "uniwrt-backup.json";
}

/// How the last nightly or requested backup went (the `controller_backups` setting).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct BackupOutcome {
    pub at: Timestamp,
    /// Why it failed, or why its copy to the extra directory did; `None` when all went well.
    pub error: Option<String>,
}

impl BackupOutcome {
    pub const KEY: &'static str = "controller_backups";
}

/// A restore waiting for the next start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RestoreState {
    None,
    /// A file is uploaded; its passphrase is still to be given.
    Uploaded {
        size_bytes: u64,
    },
    /// Unpacked and checked: the next start puts it in place.
    Ready {
        manifest: BackupManifest,
    },
}

/// What `GET v1/controller/backups` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ControllerBackupStatus {
    /// Whether an admin has set the passphrase: without it, no backup is taken.
    pub passphrase_set: bool,
    /// The backup directory and the extra one (`--backup-dir`), as the controller sees them.
    pub dir: String,
    pub extra_dir: Option<String>,
    /// Newest first.
    pub backups: Vec<ControllerBackup>,
    /// Whether one is being taken.
    pub running: bool,
    pub last: Option<BackupOutcome>,
    pub restore: RestoreState,
    /// When the nightly window opens, in minutes after the site's midnight ([`AT_MIN`]).
    pub at_min: u16,
}

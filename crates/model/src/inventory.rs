//! What the controller keeps about an adopted node besides its facts: the packages it runs and
//! its configuration backups (features/adoption.md, Path A).

use serde::{Deserialize, Serialize};
use uniwrt_core::{BackupId, NodeId, Timestamp};

/// The node's software as read at adoption.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Inventory {
    pub taken_at: Timestamp,
    /// Installed packages, by name.
    pub packages: Vec<Package>,
}

/// An installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Package {
    pub name: String,
    pub version: String,
    /// Installed on purpose (in the image's or the user's list), not only as a dependency.
    pub explicit: bool,
}

/// A configuration backup kept on the controller: the tarball `sysupgrade -b` makes, sealed at
/// rest because it holds the node's password hashes, private host keys and Wi-Fi keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Backup {
    pub id: BackupId,
    pub node: NodeId,
    pub taken_at: Timestamp,
    pub reason: BackupReason,
    pub size_bytes: u64,
    /// SHA-256 of the tarball, in hexadecimal.
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BackupReason {
    /// Taken when the node was adopted, before the controller changed anything.
    Adoption,
}

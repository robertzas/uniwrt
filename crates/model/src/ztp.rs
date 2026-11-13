//! Zero-touch provisioning (features/adoption.md, Path B; D-022, D-102): what a device flashed
//! with a ZTP image tells the controller when it calls home ([`ZtpHello`]), the pending
//! adoption inbox it lands in ([`PendingDevice`]), and the owner's pre-approvals by MAC
//! ([`ZtpApproval`]) or by a one-liner's single-use token ([`ZtpOneLiner`], Path D). Nothing
//! here is secret but a one-liner's token: the hello is unauthenticated, so the owner's
//! approval and the host key the controller pins are what adoption trusts.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use uniwrt_core::{JobId, MacAddr, Timestamp};

use crate::node::NodeRole;

/// What a ZTP device posts to `POST /api/v1/ztp/hello` (its first-boot script, every minute or
/// so until it is adopted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ZtpHello {
    /// Its identity: the label MAC (`board.json`), else its LAN bridge's.
    pub mac: MacAddr,
    /// `ubus call system board`'s `board_name`, such as `glinet,gl-mt6000`.
    pub board_name: String,
    /// Its model, for people.
    pub model: String,
    pub hostname: String,
    /// OpenWrt's version, such as `24.10.4`.
    pub release: String,
    /// Dropbear's ed25519 host key, in OpenSSH form: the key adoption pins.
    pub host_key: String,
    /// The role its image was built for; the controller infers one when `None`.
    #[serde(default)]
    pub role: Option<NodeRole>,
    /// The one-liner's token it was set up with (Path D): a valid one pre-approves it.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub token: Option<String>,
}

/// What the controller answers a hello: the script stops calling once adopted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HelloAnswer {
    pub state: HelloState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HelloState {
    /// In the inbox, waiting for the owner.
    Pending,
    /// An adoption job runs for it.
    Adopting,
    /// Adopted: the device removes its call-home service.
    Adopted,
}

/// A device in the pending adoption inbox: its latest hello, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PendingDevice {
    pub mac: MacAddr,
    pub board_name: String,
    pub model: String,
    pub hostname: String,
    pub release: String,
    /// The role its image was built for.
    pub role: Option<NodeRole>,
    /// The hello's source address: where adoption connects (SSH on port 22).
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub address: IpAddr,
    /// In OpenSSH form.
    pub host_key: String,
    /// SHA-256, as OpenSSH shows it.
    pub fingerprint: String,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    /// The latest adoption job started for it.
    pub job: Option<JobId>,
}

/// The owner's approval of a device by MAC before it calls home: its first hello starts the
/// adoption. Single-use, and it expires ([`ZtpApproval::LIFETIME`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ZtpApproval {
    pub mac: MacAddr,
    /// The display name to adopt it with; its hostname when `None`.
    pub name: Option<String>,
    /// The role to adopt it with; the hello's, else inferred, when `None`.
    pub role: Option<NodeRole>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    /// The admin who approved it.
    pub created_by: Option<String>,
}

impl ZtpApproval {
    /// Seven days (D-102).
    pub const LIFETIME: std::time::Duration = std::time::Duration::from_hours(7 * 24);
}

/// The inbox as `GET /api/v1/ztp/pending` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ZtpInbox {
    /// Newest hello first.
    pub pending: Vec<PendingDevice>,
    /// The approvals not used or expired yet, by MAC.
    pub approvals: Vec<ZtpApproval>,
}

/// A one-liner (Path D): a command to paste into an OpenWrt shell, which fetches the first-boot
/// script with a single-use token and runs it if its SHA-256 matches (D-103). The device's
/// first hello carrying the token starts its adoption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ZtpOneLiner {
    pub token: String,
    /// An access point or a bridge.
    pub role: NodeRole,
    /// The display name to adopt it with; its hostname when `None`.
    pub name: Option<String>,
    pub command: String,
    /// The script's, in hex: what the command checks.
    pub sha256: String,
    pub created_at: Timestamp,
    /// When the next step must come by: the fetch, then the hello.
    pub expires_at: Timestamp,
    pub state: OneLinerState,
    /// Who fetched the script.
    #[cfg_attr(feature = "ts", ts(as = "Option<String>"))]
    pub fetched_from: Option<IpAddr>,
    /// The device whose hello used the token.
    pub device: Option<MacAddr>,
    /// The admin who made it.
    pub created_by: Option<String>,
}

impl ZtpOneLiner {
    /// For the fetch, then again for the hello.
    pub const LIFETIME: std::time::Duration = std::time::Duration::from_mins(15);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OneLinerState {
    /// Waiting for the device to fetch the script.
    Waiting,
    /// Fetched: waiting for the device's hello.
    Fetched,
    /// A hello used it: the device is being adopted.
    Used,
    /// Not used in time, or withdrawn.
    Expired,
}

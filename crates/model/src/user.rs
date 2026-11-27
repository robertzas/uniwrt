//! Controller users and what the audit log records about them (features/security.md,
//! Controller authentication & sessions).

use serde::{Deserialize, Serialize};
use uniwrt_core::{Timestamp, UserId};

/// A user of the controller. The password hash is kept apart, by the store, and never leaves it
/// except to be verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct User {
    pub id: UserId,
    /// Lowercase; unique.
    pub username: String,
    pub role: Role,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// What a user may do: admins everything, viewers read (features/security.md, Users).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Viewer,
}

/// A login session as stored: the user it belongs to and its times. The browser holds the
/// token; the store keeps only its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user: User,
    pub created_at: Timestamp,
    /// The last request made with it, recorded at most every few minutes.
    pub last_seen_at: Timestamp,
    /// The absolute end, however active the session is.
    pub expires_at: Timestamp,
}

/// What the audit log records (features/security.md: every login, change, apply, terminal
/// session and secret view; logins first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// The first admin was created.
    Setup,
    Login,
    LoginFailed,
    Logout,
    /// A user changed their own password, or was refused for a wrong current one (the detail
    /// says which; D-153).
    Password,
    /// An admin added a user, changed one's role or password, or deleted one (the detail says
    /// which; D-153).
    Users,
    /// An admin's password was replaced from the controller's host (`uniwrt reset-admin`;
    /// D-153).
    ResetAdmin,
    /// A user started adopting a node (the detail names it and the job).
    Adopt,
    /// A user switched a node's management on or off (the detail says which, and the job).
    Manage,
    /// A user gave a client an alias or took it away (the detail names the client and alias).
    RenameClient,
    /// A user kicked a client off the Wi-Fi (the detail names it and the ban).
    KickClient,
    /// A user blocked or unblocked a client (the detail names it and says which).
    BlockClient,
    /// An admin woke a client with a Wake-on-LAN packet (the detail names it and where it was
    /// sent; D-137).
    WakeClient,
    /// A user created, edited, reset or deleted an alert rule (the detail says which and how).
    AlertRule,
    /// A user acknowledged an alert.
    AcknowledgeAlert,
    /// A user scheduled or deleted a maintenance window.
    Maintenance,
    /// A user created, edited or deleted a notifier, or changed the notification settings.
    Notifier,
    /// A user asked for neighbor scans or changed the scanning settings.
    Rf,
    /// A user changed the WAN quality monitor's targets (D-144).
    WanMonitor,
    /// A user downloaded a support bundle (D-144).
    SupportBundle,
    /// A user saved or restored the site's configuration (the detail names the revision).
    Site,
    /// A user viewed a configuration secret (the detail names it and the revision).
    RevealSecret,
    /// A user started applying a revision to the nodes (the detail names it and the job).
    Apply,
    /// A user acknowledged a node's drift, or muted or watched a package's (the detail says
    /// which).
    Drift,
    /// A user tried a WAN on the gateway (the detail names the job).
    WanTest,
    /// A user ran a speed test from the gateway (the detail names the job).
    SpeedTest,
    /// A user swapped a node's basic wpad for the full one (the detail names the job).
    Wpad,
    /// A user changed a node's role (the detail names the node and both roles).
    NodeRole,
    /// A user forgot a cable's failed backhaul probe, so the next apply probes it again (the
    /// detail names both ends).
    Backhaul,
    /// A user pre-approved a zero-touch device's MAC, withdrew it, or dismissed a device from
    /// the inbox (the detail says which; D-102).
    Ztp,
    /// A user added, changed or deleted a floor plan, replaced its image, or placed a node on it
    /// or took one off (the detail says which; D-117).
    FloorPlan,
    /// A user asked for a node's upgrade image (the detail names the node, both releases and
    /// the build; D-121).
    Firmware,
    /// A user installed a controller release or changed its automatic install (D-123).
    Controller,
    /// An admin took a node's backup, downloaded one or restored one onto the node (the detail
    /// says which; D-126).
    Backup,
    /// An admin decommissioned a node or forgot it (the detail names it and says which; D-131).
    Decommission,
    /// An admin replaced a node's device or reset it to await its re-adoption (the detail
    /// says which; D-131).
    Replace,
    /// An admin acted on a network service: asked for DNS filtering's lists again (D-133).
    Services,
    /// An admin restarted nodes (the detail names them and the job; D-137).
    Reboot,
    /// An admin changed a node's packages (the detail names the node, the changes and the job;
    /// D-139), or the feed mirror's settings, or refreshed it (D-147).
    Packages,
    /// An admin logged the gateway's Tailscale in or out (the detail says which; D-140).
    Tailscale,
    /// An admin ran a diagnostics tool on a node (the detail names the tool, its target, the
    /// node and the run; D-141).
    Diagnostics,
    /// An admin opened a terminal on a node, or it closed (the detail names the node, and how
    /// long it was open and why it closed; keystrokes aren't recorded; D-141).
    Terminal,
}

/// One audit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AuditEntry {
    pub at: Timestamp,
    pub action: AuditAction,
    /// The user acting, when known.
    pub user_id: Option<UserId>,
    /// The username given, even one that does not exist (a failed login).
    pub username: Option<String>,
    /// The client's address.
    pub ip: Option<String>,
    /// What the action was about, for people, such as `root@10.0.0.2:22, job 3`.
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AuditAction, Role};

    #[test]
    fn serde_names() {
        assert_eq!(serde_json::to_string(&Role::Admin).unwrap(), r#""admin""#);
        assert_eq!(
            serde_json::to_string(&AuditAction::LoginFailed).unwrap(),
            r#""login_failed""#
        );
    }
}

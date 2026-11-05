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

/// What a user may do. Viewers arrive in M9; until then every user is an admin.
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
    /// A user started adopting a node (the detail names it and the job).
    Adopt,
    /// A user switched a node's management on or off (the detail says which, and the job).
    Manage,
    /// A user gave a client an alias or took it away (the detail names the client and alias).
    RenameClient,
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
    /// A user saved or restored the site's configuration (the detail names the revision).
    Site,
    /// A user viewed a configuration secret (the detail names it and the revision).
    RevealSecret,
    /// A user started applying a revision to the nodes (the detail names it and the job).
    Apply,
    /// A user acknowledged a node's drift, or muted or watched a package's (the detail says
    /// which).
    Drift,
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

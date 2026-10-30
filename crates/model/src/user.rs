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

//! A forgotten admin password replaced from the controller's host (features/security.md, Users;
//! D-153): `uniwrt reset-admin [USERNAME]` gives the admin a random password, which it prints
//! once, and ends their sessions. It opens the database beside a running controller (SQLite's
//! WAL lets both write), so it needs no restart; the audit log records it.

use std::fmt;

use ring::rand::{SecureRandom as _, SystemRandom};
use uniwrt_core::Timestamp;
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::{AuditRepo as _, Store, UserChange, UserRepo as _};
use zeroize::Zeroizing;

use super::password::{PasswordCost, Passwords};

/// The new password's characters: lowercase letters and digits that can't be mistaken for one
/// another (no `i`, `l`, `o`, `0`, `1`).
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
/// Groups of [`GROUP`] characters, joined by hyphens: 20 characters, about 99 bits.
const GROUPS: usize = 4;
const GROUP: usize = 5;

/// The admin whose password was replaced, and the new password.
pub struct Reset {
    pub username: String,
    pub password: Zeroizing<String>,
}

/// Why no password was replaced.
#[derive(Debug)]
pub enum ResetError {
    /// The controller has no users: first-run setup creates the admin.
    NoUsers,
    /// No user has this name.
    Unknown(String),
    /// This user is a viewer: an admin sets their password in the UI.
    Viewer(String),
    /// Several admins and none named: these.
    Several(Vec<String>),
    Store(uniwrt_store::Error),
    Failed(String),
}

impl fmt::Display for ResetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoUsers => write!(
                f,
                "the controller has no users yet: open it in a browser to create the admin"
            ),
            Self::Unknown(name) => write!(f, "no user is named {name}"),
            Self::Viewer(name) => write!(
                f,
                "{name} is a viewer: an admin sets their password in Settings, Users"
            ),
            Self::Several(names) => write!(
                f,
                "there are several admins ({}): name the one to reset",
                names.join(", ")
            ),
            Self::Store(err) => write!(f, "{err}"),
            Self::Failed(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ResetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(err) => Some(err),
            _ => None,
        }
    }
}

impl From<uniwrt_store::Error> for ResetError {
    fn from(err: uniwrt_store::Error) -> Self {
        Self::Store(err)
    }
}

/// Gives the admin `username` (or the only admin, when `None`) a random password, ends their
/// sessions and records it in the audit log at `at`.
pub async fn reset_admin(
    store: &Store,
    username: Option<&str>,
    at: Timestamp,
) -> Result<Reset, ResetError> {
    let users = store.users().await?;
    if users.is_empty() {
        return Err(ResetError::NoUsers);
    }
    let admins = users
        .iter()
        .map(|u| &u.user)
        .filter(|u| u.role == Role::Admin);
    let user = if let Some(name) = username.map(|name| name.trim().to_ascii_lowercase()) {
        let user = users
            .iter()
            .map(|u| &u.user)
            .find(|u| u.username == name)
            .ok_or_else(|| ResetError::Unknown(name.clone()))?;
        if user.role != Role::Admin {
            return Err(ResetError::Viewer(name));
        }
        user
    } else {
        let admins: Vec<_> = admins.collect();
        let [admin] = admins.as_slice() else {
            return Err(ResetError::Several(
                admins.iter().map(|u| u.username.clone()).collect(),
            ));
        };
        *admin
    };
    let password = generate()?;
    let passwords = Passwords::new(PasswordCost::default()).map_err(|err| failed(&err))?;
    let hash = passwords
        .hash(password.clone())
        .await
        .map_err(|err| failed(&err))?;
    let change = UserChange {
        role: None,
        password_hash: Some(hash),
    };
    let user = store.update_user(user.id, change, None).await?;
    store
        .record(AuditEntry {
            at,
            action: AuditAction::ResetAdmin,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: None,
            detail: Some("uniwrt reset-admin on the controller's host".to_owned()),
        })
        .await?;
    Ok(Reset {
        username: user.username,
        password,
    })
}

fn failed(err: &crate::ApiError) -> ResetError {
    ResetError::Failed(format!("{err:?}"))
}

/// A random password from [`ALPHABET`], such as `k7wq2-…`.
fn generate() -> Result<Zeroizing<String>, ResetError> {
    let random = SystemRandom::new();
    let mut password = Zeroizing::new(String::with_capacity(GROUPS * (GROUP + 1)));
    let mut byte = [0];
    let mut chosen = 0;
    while chosen < GROUPS * GROUP {
        random
            .fill(&mut byte)
            .map_err(|_| ResetError::Failed("the random number generator failed".to_owned()))?;
        // Rejection sampling: only bytes below a multiple of the alphabet's size, so each
        // character is equally likely.
        let limit = 256 - 256 % ALPHABET.len();
        if usize::from(byte[0]) >= limit {
            continue;
        }
        if chosen > 0 && chosen % GROUP == 0 {
            password.push('-');
        }
        password.push(char::from(ALPHABET[usize::from(byte[0]) % ALPHABET.len()]));
        chosen += 1;
    }
    Ok(password)
}

#[cfg(test)]
#[path = "reset_tests.rs"]
mod tests;

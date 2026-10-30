//! Repository traits: the store's API by concern. `Store` implements each; services depend on the
//! traits, so their tests can substitute fakes.

mod audit;
mod facts;
mod host_keys;
mod nodes;
mod sessions;
mod users;

use uniwrt_core::{NodeId, Timestamp, UserId};
use uniwrt_model::{
    facts::NodeFacts,
    node::{HostKeyPin, NewNode, Node},
    user::{AuditEntry, Role, Session, User},
};
use zeroize::Zeroizing;

use crate::Result;

/// The node inventory.
pub trait NodeRepo: Send + Sync {
    /// Adds a node in observe mode. A node with the same MAC is an
    /// [`Error::Conflict`](crate::Error::Conflict).
    fn create_node(&self, node: NewNode) -> impl Future<Output = Result<Node>> + Send;

    fn node(&self, id: NodeId) -> impl Future<Output = Result<Option<Node>>> + Send;

    /// All nodes, oldest first.
    fn nodes(&self) -> impl Future<Output = Result<Vec<Node>>> + Send;

    /// Removes a node and everything stored about it; false when there was no such node.
    fn delete_node(&self, id: NodeId) -> impl Future<Output = Result<bool>> + Send;
}

/// The latest facts snapshot of each node.
pub trait FactsRepo: Send + Sync {
    /// Replaces the node's snapshot, unless the stored one was collected later. An unknown node
    /// is an [`Error::NotFound`](crate::Error::NotFound).
    fn save_facts(
        &self,
        node: NodeId,
        facts: &NodeFacts,
    ) -> impl Future<Output = Result<()>> + Send;

    fn latest_facts(&self, node: NodeId) -> impl Future<Output = Result<Option<NodeFacts>>> + Send;
}

/// The SSH host key pinned for each node.
pub trait HostKeyRepo: Send + Sync {
    /// Pins the node's host key, replacing the previous pin (an admin re-trusts a reflashed
    /// node). An unknown node is an [`Error::NotFound`](crate::Error::NotFound).
    fn pin_host_key(
        &self,
        node: NodeId,
        pin: HostKeyPin,
    ) -> impl Future<Output = Result<()>> + Send;

    fn host_key(&self, node: NodeId) -> impl Future<Output = Result<Option<HostKeyPin>>> + Send;
}

/// A secret's bytes, wiped from memory when dropped.
pub type Secret = Zeroizing<Vec<u8>>;

/// Secrets by name, such as `controller/ssh_key`, sealed at rest ([`Vault`](crate::Vault)).
pub trait VaultRepo: Send + Sync {
    /// Stores a secret, replacing any with the same name.
    fn put_secret(&self, name: &str, value: Secret) -> impl Future<Output = Result<()>> + Send;

    fn secret(&self, name: &str) -> impl Future<Output = Result<Option<Secret>>> + Send;

    /// Removes a secret; false when there was no such secret.
    fn delete_secret(&self, name: &str) -> impl Future<Output = Result<bool>> + Send;
}

/// Controller users and their password hashes.
pub trait UserRepo: Send + Sync {
    /// Creates the first user; once any user exists, an [`Error::Conflict`](crate::Error::Conflict),
    /// so two concurrent setups cannot both succeed. `username` must be lowercase.
    fn create_first_user(
        &self,
        username: &str,
        role: Role,
        password_hash: &str,
    ) -> impl Future<Output = Result<User>> + Send;

    fn has_users(&self) -> impl Future<Output = Result<bool>> + Send;

    fn user(&self, id: UserId) -> impl Future<Output = Result<Option<User>>> + Send;

    /// The user named `username` (lowercase) with their password hash, to verify a login.
    fn credentials(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<Option<(User, String)>>> + Send;
}

/// A new login session, keyed by the SHA-256 of its token.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub token_sha256: [u8; 32],
    pub user: UserId,
    pub expires_at: Timestamp,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Login sessions.
pub trait SessionRepo: Send + Sync {
    /// Stores a session, created and last seen now. An unknown user is an
    /// [`Error::NotFound`](crate::Error::NotFound).
    fn create_session(&self, session: NewSession) -> impl Future<Output = Result<()>> + Send;

    /// The session with this token hash and its user, expired or not: the caller judges.
    fn session(
        &self,
        token_sha256: [u8; 32],
    ) -> impl Future<Output = Result<Option<Session>>> + Send;

    /// Records a request made with the session now.
    fn touch_session(&self, token_sha256: [u8; 32]) -> impl Future<Output = Result<()>> + Send;

    /// Ends a session; false when there was none.
    fn delete_session(&self, token_sha256: [u8; 32]) -> impl Future<Output = Result<bool>> + Send;

    /// Deletes the sessions past their absolute end or last seen before `idle_since`; returns
    /// how many.
    fn delete_stale_sessions(
        &self,
        idle_since: Timestamp,
    ) -> impl Future<Output = Result<usize>> + Send;
}

/// The audit log.
pub trait AuditRepo: Send + Sync {
    fn record(&self, entry: AuditEntry) -> impl Future<Output = Result<()>> + Send;

    /// The latest `limit` entries, newest first.
    fn audit_log(&self, limit: usize) -> impl Future<Output = Result<Vec<AuditEntry>>> + Send;
}

//! Repository traits: the store's API by concern. `Store` implements each; services depend on the
//! traits, so their tests can substitute fakes.

mod adoption;
mod audit;
mod facts;
mod host_keys;
mod jobs;
mod nodes;
mod sessions;
mod topology;
mod users;

use uniwrt_core::{JobId, NodeId, Timestamp, UserId};
use uniwrt_model::{
    facts::NodeFacts,
    inventory::{Backup, Inventory},
    job::{Job, JobError, NewJob},
    node::{HostKeyPin, NewNode, Node},
    user::{AuditEntry, Role, Session, User},
};
use zeroize::Zeroizing;

pub use adoption::Adoption;

use crate::Result;

/// The node inventory.
pub trait NodeRepo: Send + Sync {
    /// Adds a node in observe mode. A node with the same MAC is an
    /// [`Error::Conflict`](crate::Error::Conflict).
    fn create_node(&self, node: NewNode) -> impl Future<Output = Result<Node>> + Send;

    /// Adds an adopted node in observe mode with its pinned host key, first facts, inventory and
    /// backup, all or nothing. A node with the same MAC is an
    /// [`Error::Conflict`](crate::Error::Conflict).
    fn adopt_node(&self, adoption: Adoption) -> impl Future<Output = Result<Node>> + Send;

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

/// What adoption keeps about a node besides its facts.
pub trait InventoryRepo: Send + Sync {
    fn inventory(&self, node: NodeId) -> impl Future<Output = Result<Option<Inventory>>> + Send;

    /// The node's configuration backups, newest first; [`Vault::open_backup`](crate::Vault::open_backup)
    /// reads one.
    fn backups(&self, node: NodeId) -> impl Future<Output = Result<Vec<Backup>>> + Send;
}

/// Jobs as history: each is saved as it progresses (D-042).
pub trait JobRepo: Send + Sync {
    /// Records a job starting now, every step pending.
    fn create_job(&self, job: NewJob) -> impl Future<Output = Result<Job>> + Send;

    /// Saves a job's progress: its state, node, steps, error and end. An unknown job is an
    /// [`Error::NotFound`](crate::Error::NotFound).
    fn update_job(&self, job: &Job) -> impl Future<Output = Result<()>> + Send;

    fn job(&self, id: JobId) -> impl Future<Output = Result<Option<Job>>> + Send;

    /// The latest `limit` jobs, newest first.
    fn jobs(&self, limit: usize) -> impl Future<Output = Result<Vec<Job>>> + Send;

    /// Fails every job still running, and its running step, with `error`: at startup, the jobs
    /// that a stop interrupted. Returns how many.
    fn interrupt_jobs(&self, error: JobError) -> impl Future<Output = Result<usize>> + Send;
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

/// One recorded state of the topology, as JSON the engine encodes: a keyframe (a whole
/// topology) or a delta from the frame before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyFrame {
    pub at: Timestamp,
    pub keyframe: bool,
    pub data: String,
}

/// A vertex of the topology view pinned at a place, in the view's world coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub vertex: String,
    pub x: f64,
    pub y: f64,
}

/// The topology's history, for the replay, and the topology view's layout.
pub trait TopologyRepo: Send + Sync {
    /// Appends a frame.
    fn record_topology(&self, frame: TopologyFrame) -> impl Future<Output = Result<()>> + Send;

    /// The frames that make the topology at `at`: the newest keyframe at or before it, then the
    /// deltas after it up to `at`, in the order recorded. Empty before the first keyframe.
    fn topology_frames(
        &self,
        at: Timestamp,
    ) -> impl Future<Output = Result<Vec<TopologyFrame>>> + Send;

    /// When each frame was recorded and whether it is a keyframe, in the order recorded.
    fn topology_times(&self) -> impl Future<Output = Result<Vec<(Timestamp, bool)>>> + Send;

    /// Deletes the frames that no topology at or after `before` needs: those older than the
    /// newest keyframe at or before it. Returns how many.
    fn prune_topology(&self, before: Timestamp) -> impl Future<Output = Result<usize>> + Send;

    /// Every pin, by vertex id.
    fn pins(&self) -> impl Future<Output = Result<Vec<Pin>>> + Send;

    /// Pins a vertex, or moves its pin; false (and nothing pinned) when the vertex is new and
    /// `max` vertices are pinned already.
    fn pin(&self, pin: Pin, max: usize) -> impl Future<Output = Result<bool>> + Send;

    /// False when the vertex was not pinned.
    fn unpin(&self, vertex: &str) -> impl Future<Output = Result<bool>> + Send;

    /// Returns how many pins there were.
    fn unpin_all(&self) -> impl Future<Output = Result<usize>> + Send;
}

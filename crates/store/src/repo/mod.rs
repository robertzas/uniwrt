//! Repository traits: the store's API by concern. `Store` implements each; services depend on the
//! traits, so their tests can substitute fakes.

mod adoption;
mod alerts;
mod applies;
mod audit;
mod backhaul;
mod clients;
mod drift;
mod events;
mod facts;
mod floors;
#[cfg(test)]
mod floors_tests;
mod host_keys;
mod jobs;
mod logs;
mod metrics;
mod nodes;
mod notifiers;
mod revisions;
#[cfg(test)]
mod revisions_tests;
mod rf;
mod sessions;
mod speed;
mod topology;
mod users;
mod ztp;

use uniwrt_core::{
    AlertId, EventId, FloorId, JobId, LogId, MacAddr, NodeId, NotifierId, RevisionId, RuleId,
    ScanId, SearchId, Timestamp, UserId, WindowId,
};
use uniwrt_model::{
    alerts::{
        Acknowledgement, AlertFilter, AlertRecord, AlertRule, AlertWrite, MaintenanceWindow,
        NewWindow, RuleSpec,
    },
    apply::{Applied, AppliedRevision, BackhaulFailure, CableEnd},
    client::{ClientRecord, ClientSession},
    drift::{Confirmed, DriftMute, DriftRecord},
    events::{EventFilter, EventRecord, EventWrite},
    facts::NodeFacts,
    floor::{Floor, FloorFields, FloorImage, ImageKind, Placement, Wall},
    inventory::{Backup, Inventory},
    job::{Job, JobError, NewJob},
    log::{LogBucket, LogFilter, LogRecord, LogSources, NewLog, SavedSearch},
    metric::{Entity, Metric, Point, Resolution, Sample},
    node::{HostKeyPin, NewNode, Node, NodeMode, NodeRole},
    notify::{Notifier, NotifierConfig},
    rf::{NewScan, Scan, ScanSummary},
    site::revision::{NewRevision, Revision, RevisionMeta},
    speed::SpeedTest,
    user::{AuditEntry, Role, Session, User},
};
use zeroize::Zeroizing;

pub use adoption::Adoption;
pub use ztp::ZtpRepo;

use crate::{Result, SealedSecret};

/// The revision each node holds (features/lifecycle.md, Render → diff → apply).
pub trait ApplyRepo: Send + Sync {
    /// Records that `applied.nodes` hold `applied.revision` (those with `holds`; the others
    /// only their packages, D-081), and moves each whose address changed, all or nothing. A
    /// node removed meanwhile is left out.
    fn record_applied(&self, applied: Applied) -> impl Future<Output = Result<()>> + Send;

    /// The revision each node holds, by node id.
    fn applied_revisions(&self) -> impl Future<Output = Result<Vec<AppliedRevision>>> + Send;
}

/// Cables that failed the wired backhaul's probe (features/mesh.md, Wired backhaul; D-097), one
/// per cable whichever end names it first.
pub trait BackhaulRepo: Send + Sync {
    /// Records the cable's failure, replacing an earlier one of the same cable.
    fn record_backhaul_failure(
        &self,
        failure: BackhaulFailure,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Every failed cable.
    fn backhaul_failures(&self) -> impl Future<Output = Result<Vec<BackhaulFailure>>> + Send;

    /// Forgets the cable's failure, so the next apply probes it again; whether it had one.
    fn forget_backhaul_failure(
        &self,
        ends: [CableEnd; 2],
    ) -> impl Future<Output = Result<bool>> + Send;
}

/// A floor's new image, checked by the caller: its kind and size come from its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFloorImage {
    pub kind: ImageKind,
    pub width_px: u32,
    pub height_px: u32,
    pub data: Vec<u8>,
}

/// Floor plans (features/wifi.md, Coverage & floor plans; D-117): floors by level, each with its
/// image, scale and the nodes placed on it. Every write stamps the floor's `updated_at`.
pub trait FloorRepo: Send + Sync {
    /// Every floor by level, with its placements but without its image's data.
    fn floors(&self) -> impl Future<Output = Result<Vec<Floor>>> + Send;

    fn floor(&self, id: FloorId) -> impl Future<Output = Result<Option<Floor>>> + Send;

    /// Adds a floor with the checked `fields`; [`Error::Conflict`] when another floor has its
    /// level (`what` names the level) or `max` floors exist (`what` is "the most floors").
    ///
    /// [`Error::Conflict`]: crate::Error::Conflict
    fn create_floor(
        &self,
        fields: FloorFields,
        max: usize,
    ) -> impl Future<Output = Result<Floor>> + Send;

    /// Replaces a floor's own fields; None when it doesn't exist, a conflict as for
    /// [`create_floor`](Self::create_floor) when another floor has the level.
    fn update_floor(
        &self,
        id: FloorId,
        fields: FloorFields,
    ) -> impl Future<Output = Result<Option<Floor>>> + Send;

    /// Deletes a floor with its image and placements; false when it didn't exist.
    fn delete_floor(&self, id: FloorId) -> impl Future<Output = Result<bool>> + Send;

    /// Replaces a floor's walls, checked by the caller; None when the floor doesn't exist.
    fn set_floor_walls(
        &self,
        id: FloorId,
        walls: Vec<Wall>,
    ) -> impl Future<Output = Result<Option<Floor>>> + Send;

    /// Replaces a floor's image and bumps its version. The scale's line, the walls and the
    /// placements are stretched to the new size on each axis, as for the same drawing at another resolution.
    /// None when the floor doesn't exist.
    fn set_floor_image(
        &self,
        id: FloorId,
        image: NewFloorImage,
    ) -> impl Future<Output = Result<Option<Floor>>> + Send;

    /// A floor's image and its data; None without a floor or an image.
    fn floor_image(
        &self,
        id: FloorId,
    ) -> impl Future<Output = Result<Option<(FloorImage, Vec<u8>)>>> + Send;

    /// Places a node on a floor, moving it from any other; None when the floor doesn't exist,
    /// [`Error::NotFound`](crate::Error::NotFound) when the node doesn't. Positions are the
    /// caller's to check.
    fn place_node(
        &self,
        floor: FloorId,
        placement: Placement,
    ) -> impl Future<Output = Result<Option<Floor>>> + Send;

    /// Takes a node off its floor; false when it was on none.
    fn unplace_node(&self, node: NodeId) -> impl Future<Output = Result<bool>> + Send;
}

/// Drift (features/lifecycle.md, Drift): what the controller confirmed on each node, the
/// packages that differ from it, and the packages whose drift isn't watched.
pub trait DriftRepo: Send + Sync {
    /// The managed packages as the controller confirmed them: on `node`, or on every node.
    fn confirmed(
        &self,
        node: Option<NodeId>,
    ) -> impl Future<Output = Result<Vec<Confirmed>>> + Send;

    /// Takes the node's file as confirmed after all, as it reads now: it changed only in form.
    /// Nothing happens without a confirmed row.
    fn reconfirm(
        &self,
        node: NodeId,
        package: String,
        sha256: String,
        text: String,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Every drift, by node and package.
    fn drifts(&self) -> impl Future<Output = Result<Vec<DriftRecord>>> + Send;

    /// Stores a drift, replacing the node's record for the package; a removed node is left
    /// out.
    fn put_drift(&self, record: DriftRecord) -> impl Future<Output = Result<()>> + Send;

    /// Forgets a drift; whether there was one.
    fn delete_drift(
        &self,
        node: NodeId,
        package: String,
    ) -> impl Future<Output = Result<bool>> + Send;

    fn drift_mutes(&self) -> impl Future<Output = Result<Vec<DriftMute>>> + Send;

    /// Mutes a package's drift (`muted`, by the user named) or watches it again; whether that
    /// changed anything.
    fn set_drift_mute(
        &self,
        node: NodeId,
        package: String,
        muted: bool,
        by: Option<String>,
    ) -> impl Future<Output = Result<bool>> + Send;
}

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

    /// Sets a node's mode (management on or off); the node as it is now, or `None` when there
    /// is no such node. Observe mode forgets what the controller confirmed on it and its drift.
    fn set_node_mode(
        &self,
        id: NodeId,
        mode: NodeMode,
    ) -> impl Future<Output = Result<Option<Node>>> + Send;

    /// Sets a node's role (D-100); the node as it is now, or `None` when there is no such
    /// node. What the role means for its configuration takes effect with the next apply.
    fn set_node_role(
        &self,
        id: NodeId,
        role: NodeRole,
    ) -> impl Future<Output = Result<Option<Node>>> + Send;
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

/// Revisions of the site's desired state: one linear history (D-065).
pub trait RevisionRepo: Send + Sync {
    /// The latest revision, if any.
    fn head_revision(&self) -> impl Future<Output = Result<Option<Revision>>> + Send;

    fn revision(&self, id: RevisionId) -> impl Future<Output = Result<Option<Revision>>> + Send;

    /// Up to `limit` revisions older than `before` (from the latest when `None`), newest first.
    fn revisions(
        &self,
        before: Option<RevisionId>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<RevisionMeta>>> + Send;

    /// Commits `new` as the latest revision, with the diff from its parent and the
    /// configuration secrets it introduces, all or nothing. Fails with
    /// [`Error::NotLatest`](crate::Error::NotLatest) when `new.base` is no longer the latest,
    /// and with [`Error::UnknownSecret`](crate::Error::UnknownSecret) when the document refers to
    /// a secret that neither `secrets`, its parent nor the revision it restores has.
    fn commit_revision(
        &self,
        new: NewRevision,
        secrets: Vec<SealedSecret>,
    ) -> impl Future<Output = Result<RevisionMeta>> + Send;
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

/// Speed tests from the gateway (features/wan.md, SQM; D-076), the newest 500.
pub trait SpeedTestRepo: Send + Sync {
    fn record_speed_test(&self, test: SpeedTest) -> impl Future<Output = Result<()>> + Send;

    /// The latest `limit` tests, newest first.
    fn speed_tests(&self, limit: usize) -> impl Future<Output = Result<Vec<SpeedTest>>> + Send;
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

/// Clients (features/observability.md, Clients): what the controller keeps about each one and
/// where it was connected.
pub trait ClientRepo: Send + Sync {
    /// Every client, by MAC address.
    fn clients(&self) -> impl Future<Output = Result<Vec<ClientRecord>>> + Send;

    /// Saves records: new ones whole; for known ones, the hostname (when the record has one)
    /// and the earliest first and latest last sighting, keeping the stored alias.
    fn save_clients(&self, records: Vec<ClientRecord>) -> impl Future<Output = Result<()>> + Send;

    /// Saves a record like [`save_clients`](Self::save_clients), with what users chose for it:
    /// its alias and when it was blocked (or none).
    fn save_choices(&self, record: ClientRecord) -> impl Future<Output = Result<()>> + Send;

    /// Saves sessions, each replacing the one of its client with the same start.
    fn save_sessions(
        &self,
        sessions: Vec<ClientSession>,
    ) -> impl Future<Output = Result<()>> + Send;

    /// A client's sessions that go on or ended at or after `since`, oldest first.
    fn client_sessions(
        &self,
        mac: MacAddr,
        since: Timestamp,
    ) -> impl Future<Output = Result<Vec<ClientSession>>> + Send;

    /// Each client's latest session, by MAC address.
    fn latest_sessions(&self) -> impl Future<Output = Result<Vec<ClientSession>>> + Send;

    /// Deletes the sessions that ended before `before`, and the clients without an alias last
    /// seen before it with all their sessions. Returns how many clients and sessions.
    fn prune_clients(
        &self,
        before: Timestamp,
    ) -> impl Future<Output = Result<(usize, usize)>> + Send;
}

/// Before when each resolution of the metrics is deleted ([`MetricsRepo::prune_metrics`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsPrune {
    pub raw: Timestamp,
    pub minute: Timestamp,
    pub hour: Timestamp,
}

/// Metrics (features/observability.md, Metrics): series by entity and metric, as raw windows
/// and their 1-minute and 1-hour rollups.
pub trait MetricsRepo: Send + Sync {
    /// Records samples, each replacing the value of its series' window.
    fn record_metrics(&self, samples: Vec<Sample>) -> impl Future<Output = Result<()>> + Send;

    /// A series' points from `from` (included) to `to` (excluded) at `resolution`, oldest
    /// first, with the tail that rollups have not reached yet read from finer resolutions.
    fn metric_points(
        &self,
        entity: &Entity,
        metric: Metric,
        resolution: Resolution,
        from: Timestamp,
        to: Timestamp,
    ) -> impl Future<Output = Result<Vec<Point>>> + Send;

    /// The metrics kept for each entity: every entity, or `under` and the entities below it
    /// (a node's devices and radios), by entity.
    fn metric_series(
        &self,
        under: Option<Entity>,
    ) -> impl Future<Output = Result<Vec<(Entity, Vec<Metric>)>>> + Send;

    /// Rolls raw windows up into the minutes that ended by `until`, and minutes into the hours
    /// they complete, from where the last call stopped (from `earliest` the first time). One
    /// call does a bounded amount; it returns whether more is due.
    fn roll_up_metrics(
        &self,
        until: Timestamp,
        earliest: Timestamp,
    ) -> impl Future<Output = Result<bool>> + Send;

    /// Deletes what each resolution holds from before its time, and the series left empty.
    /// Returns how many points.
    fn prune_metrics(&self, before: MetricsPrune) -> impl Future<Output = Result<usize>> + Send;
}

/// Before when, and above what size, logs are deleted ([`LogRepo::prune_logs`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogsPrune {
    /// Entries that arrived before this go.
    pub before: Timestamp,
    /// The oldest entries go while the logs take more than this, estimated from the newest
    /// entries' size.
    pub max_bytes: u64,
}

/// Logs (features/observability.md, Logs): entries with a full-text index, the hosts and
/// programs that logged, and saved searches.
pub trait LogRepo: Send + Sync {
    /// Stores entries in order; returns them as stored, with their ids.
    fn record_logs(&self, logs: Vec<NewLog>)
    -> impl Future<Output = Result<Vec<LogRecord>>> + Send;

    /// Up to `limit` entries that pass `filter`, newest first (by time, then id), from before
    /// `before` (a time and id) when given.
    fn logs(
        &self,
        filter: LogFilter,
        before: Option<(Timestamp, LogId)>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<LogRecord>>> + Send;

    /// The entry `id` with up to `around` entries logged before it and after it, from every
    /// source, oldest first; empty when there is no such entry.
    fn log_context(
        &self,
        id: LogId,
        around: usize,
    ) -> impl Future<Output = Result<Vec<LogRecord>>> + Send;

    /// How many entries that pass `filter` were logged in each `step` from `from` to `to`, by
    /// severity: every bucket, empty ones too.
    fn log_histogram(
        &self,
        filter: LogFilter,
        from: Timestamp,
        to: Timestamp,
        step: std::time::Duration,
    ) -> impl Future<Output = Result<Vec<LogBucket>>> + Send;

    /// The newest entry of a node, to resume after.
    fn last_log(&self, node: NodeId) -> impl Future<Output = Result<Option<LogRecord>>> + Send;

    /// The hosts and programs that logged since `since`, by name.
    fn log_sources(&self, since: Timestamp) -> impl Future<Output = Result<LogSources>> + Send;

    /// Deletes the oldest entries: those that arrived before `prune.before`, then more while
    /// the logs are larger than `prune.max_bytes`. One call deletes a bounded number; it
    /// returns how many, so a caller repeats while it deleted some.
    fn prune_logs(&self, prune: LogsPrune) -> impl Future<Output = Result<usize>> + Send;

    fn saved_searches(&self) -> impl Future<Output = Result<Vec<SavedSearch>>> + Send;

    /// Saves a search; a name already used is an [`Error::Conflict`](crate::Error::Conflict).
    fn save_search(
        &self,
        name: String,
        filter: LogFilter,
        by: Option<UserId>,
    ) -> impl Future<Output = Result<SavedSearch>> + Send;

    /// Deletes a saved search; false when there was none.
    fn delete_search(&self, id: SearchId) -> impl Future<Output = Result<bool>> + Send;
}

/// How far [`EventRepo::prune_events`] goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventsPrune {
    /// Events whose latest repeat is older go.
    pub before: Timestamp,
    /// The oldest events go while there are more.
    pub max_events: u64,
}

/// Events (features/observability.md, Events): the network's timeline.
pub trait EventRepo: Send + Sync {
    /// Stores new events and repeats of recorded ones, in order; returns each as stored, with
    /// its id (`None` for a repeat of an event no longer stored).
    fn record_events(
        &self,
        writes: Vec<EventWrite>,
    ) -> impl Future<Output = Result<Vec<Option<EventRecord>>>> + Send;

    /// Up to `limit` events that pass `filter`, newest first (by time, then id), from before
    /// `before` (a time and id) when given.
    fn events(
        &self,
        filter: EventFilter,
        before: Option<(Timestamp, EventId)>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<EventRecord>>> + Send;

    /// The conditions whose latest event raised them, by key, with when it did.
    fn raised_conditions(&self) -> impl Future<Output = Result<Vec<(String, Timestamp)>>> + Send;

    /// Deletes the oldest events: those last seen before `prune.before`, then more while there
    /// are more than `prune.max_events`. One call deletes a bounded number; it returns how
    /// many, so a caller repeats while it deleted some.
    fn prune_events(&self, prune: EventsPrune) -> impl Future<Output = Result<usize>> + Send;
}

/// Alerts (features/observability.md, Alerts): their rules, the alerts they fired and the
/// maintenance windows that silence them.
pub trait AlertRepo: Send + Sync {
    /// Every rule, by id.
    fn alert_rules(&self) -> impl Future<Output = Result<Vec<AlertRule>>> + Send;

    fn alert_rule(&self, id: RuleId) -> impl Future<Output = Result<Option<AlertRule>>> + Send;

    /// Stores the built-in rules (`preset`, spec) not stored yet; returns how many.
    fn seed_alert_rules(
        &self,
        presets: Vec<(String, RuleSpec)>,
    ) -> impl Future<Output = Result<usize>> + Send;

    fn create_alert_rule(&self, spec: RuleSpec) -> impl Future<Output = Result<AlertRule>> + Send;

    /// Replaces a rule's spec; `None` when there is no such rule.
    fn update_alert_rule(
        &self,
        id: RuleId,
        spec: RuleSpec,
    ) -> impl Future<Output = Result<Option<AlertRule>>> + Send;

    /// Deletes one of the user's rules (not a built-in one); whether it did.
    fn delete_alert_rule(&self, id: RuleId) -> impl Future<Output = Result<bool>> + Send;

    /// Applies the alert engine's writes in order; returns each alert as stored (a write to an
    /// alert no longer stored is skipped).
    fn record_alerts(
        &self,
        writes: Vec<AlertWrite>,
    ) -> impl Future<Output = Result<Vec<AlertRecord>>> + Send;

    /// Up to `limit` alerts that pass `filter`, newest first (by start, then id), from before
    /// `before` (a time and id) when given.
    fn alerts(
        &self,
        filter: AlertFilter,
        before: Option<(Timestamp, AlertId)>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<AlertRecord>>> + Send;

    fn alert(&self, id: AlertId) -> impl Future<Output = Result<Option<AlertRecord>>> + Send;

    /// What the alert engine resumes: alerts firing, and the latest of each rule and subject
    /// resolved since `since`.
    fn alerts_to_resume(
        &self,
        since: Timestamp,
    ) -> impl Future<Output = Result<Vec<AlertRecord>>> + Send;

    /// Acknowledges an alert that still fires; `None` when there is no such alert or it
    /// resolved.
    fn acknowledge_alert(
        &self,
        id: AlertId,
        ack: Acknowledgement,
    ) -> impl Future<Output = Result<Option<AlertRecord>>> + Send;

    /// Deletes alerts resolved before `before`; returns how many.
    fn prune_alerts(&self, before: Timestamp) -> impl Future<Output = Result<usize>> + Send;

    /// The maintenance windows that end after `after`, by start.
    fn maintenance_windows(
        &self,
        after: Timestamp,
    ) -> impl Future<Output = Result<Vec<MaintenanceWindow>>> + Send;

    fn create_maintenance_window(
        &self,
        window: NewWindow,
        created_by: Option<UserId>,
    ) -> impl Future<Output = Result<MaintenanceWindow>> + Send;

    /// Deletes a window (ending it, when it is on); whether it did.
    fn delete_maintenance_window(&self, id: WindowId) -> impl Future<Output = Result<bool>> + Send;
}

/// Notifiers (features/observability.md, Notifications): where alerts are sent. Their secrets
/// are the vault's.
pub trait NotifierRepo: Send + Sync {
    /// Every notifier, by id.
    fn notifiers(&self) -> impl Future<Output = Result<Vec<Notifier>>> + Send;

    fn notifier(&self, id: NotifierId) -> impl Future<Output = Result<Option<Notifier>>> + Send;

    fn create_notifier(
        &self,
        config: NotifierConfig,
    ) -> impl Future<Output = Result<Notifier>> + Send;

    /// Replaces a notifier's configuration; `None` when there is no such notifier.
    fn update_notifier(
        &self,
        id: NotifierId,
        config: NotifierConfig,
    ) -> impl Future<Output = Result<Option<Notifier>>> + Send;

    /// Whether there was such a notifier.
    fn delete_notifier(&self, id: NotifierId) -> impl Future<Output = Result<bool>> + Send;

    /// Records how a delivery went at `at`: a success, or the error.
    fn record_delivery(
        &self,
        id: NotifierId,
        at: Timestamp,
        error: Option<String>,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// The controller's settings by key, each a JSON value.
/// RF insight (features/wifi.md, RF insight): radios' neighbor scans.
pub trait RfRepo: Send + Sync {
    /// Stores a scan; returns it with its id.
    fn add_scan(&self, scan: NewScan) -> impl Future<Output = Result<Scan>> + Send;

    /// The latest scan of each radio (by node and radio name) from `since` on.
    fn latest_scans(&self, since: Timestamp) -> impl Future<Output = Result<Vec<Scan>>> + Send;

    /// Up to `limit` scans, newest first, of one node (and one of its radios) when given, from
    /// before `before` when given.
    fn scans(
        &self,
        node: Option<NodeId>,
        radio: Option<String>,
        before: Option<ScanId>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<ScanSummary>>> + Send;

    fn scan(&self, id: ScanId) -> impl Future<Output = Result<Option<Scan>>> + Send;

    /// Deletes the scans from before `before`; returns how many.
    fn prune_scans(&self, before: Timestamp) -> impl Future<Output = Result<usize>> + Send;
}

pub trait SettingsRepo: Send + Sync {
    fn setting(
        &self,
        key: &'static str,
    ) -> impl Future<Output = Result<Option<serde_json::Value>>> + Send;

    /// Stores a setting, replacing any.
    fn put_setting(
        &self,
        key: &'static str,
        value: serde_json::Value,
    ) -> impl Future<Output = Result<()>> + Send;
}

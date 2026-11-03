//! The engine: an actor per node over its `NodeLink`, the live state of the fleet, its topology,
//! the event bus, telemetry intervals that follow interest, snapshots, and each node's status.
//!
//! - **API:** [`Engine`] ([`add`](Engine::add) a node with its [`Identity`] (role and adoption
//!   MAC) and link, [`remove`](Engine::remove)
//!   it, read it with [`node`](Engine::node) and [`all`](Engine::all) as [`NodeView`]s, take its
//!   [`link`](Engine::link) for requests of one's own, [`subscribe`](Engine::subscribe) to the
//!   bus's [`Event`]s, read the [`topology`](Engine::topology)); [`Fleet`], its link-free face
//!   for the API (`Arc<dyn Fleet>`);
//!   [`Interest`] guards from [`Engine::focus`] (fast intervals while the UI has the node open)
//!   and [`Engine::follow_logs`] (the node's log, as [`Event::Log`]);
//!   [`EngineConfig`] with the [`Intervals`] of each collect set; [`Snapshots`] (the latest facts
//!   of each node, saved through `FactsRepo`); [`TopologyHistory`] (the topology's history,
//!   recorded through `TopologyRepo`: a keyframe every hour, deltas at most every 30 s, 24 h
//!   kept; [`HistoryConfig`]) and [`replay`], which turns its frames back into a topology;
//!   the clients ([`Engine::clients`], [`Engine::client`], [`Engine::rename_client`]) and
//!   [`ClientRecorder`], which restores their book and keeps it through `ClientRepo`;
//!   [`LogRecorder`] ([`LogsConfig`]), which has every node's log followed by its agent until
//!   syslog delivers it (D-059), takes in other hosts' syslog, annotates and stores the entries
//!   through `LogRepo` (announced as [`Event::Logged`]), and prunes them (14 days, 1 GiB);
//!   [`EventRecorder`] ([`EventsConfig`]), which derives events (`uniwrt_events`) from the bus
//!   and the job runner, folds repeats, stores them through `EventRepo` (announced as
//!   [`Event::Recorded`]) and prunes them (60 days, 200,000 events); [`AlertEngine`]
//!   ([`AlertsConfig`]), which runs the alert rules (`uniwrt_events::alerts`) over recorded
//!   events and the metric windows ([`Event::Sampled`]), silences alerts while jobs run and in
//!   maintenance windows, stores them through `AlertRepo` (announced as [`Event::Alerts`]) and
//!   prunes them (90 days after they resolve); [`Alerts`] is the API's handle on it;
//!   [`Notifications`] ([`NotifyConfig`]) sends what the alerts' notifiers should hear of
//!   (`uniwrt_notify`: opened, reopened, reminders, resolved; digests of what arrives together;
//!   retries), with their secrets from the vault, and records how each delivery went;
//!   [`Notify`] is the API's handle on it (reload, test).
//! - **Actors** (ARCHITECTURE.md, Process structure): one task per node, restarted with backoff
//!   if it panics. After each hello it subscribes to the agent's topics, collects every set,
//!   and schedules ticks on the node's plan (15 s base, 2 s focused; D-037); an agent without an
//!   event loop, or one that refuses ticks, is polled with `collect` instead. A pushed event is
//!   announced at once and collects the set it touches again after a short delay. Roams are
//!   inferred across nodes from station events ([`uniwrt_model::event::Roam`]).
//! - **Topology** (`uniwrt_topo`): built again from the live state 1 s after a node's facts or
//!   status change (the changes of that second share one build), against the previous build for
//!   traffic rates, and announced as [`Event::Topology`] when it changed.
//! - **Clients** (`uniwrt_clients`): each build updates the clients' book (sessions, samples,
//!   scores), announced as [`Event::Clients`]; the recorder saves its records and sessions,
//!   restores them at start, and prunes clients and sessions after 30 days.
//! - **Status** (`model::status`): offline while the link is not up (connecting, unreachable,
//!   quarantined), degraded while the agent is not running or its telemetry is partial (failed
//!   sets or requests) or late (none for three of its fastest intervals, at least 30 s: the
//!   agent is then asked for its pushes again), else online.
//! - **Invariants:** an actor sends its agent one request at a time. Facts are replaced, never
//!   mutated: readers hold `Arc`s, and [`Event::Facts`] goes out only when a part changed. A
//!   removed node's actor has stopped before [`Engine::remove`] returns. Timers use tokio's
//!   clock and timestamps the injected `Clock`.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-node` (the link, the ops and the
//!   parsers), `uniwrt-store` (`FactsRepo`, `TopologyRepo`, `ClientRepo`, `MetricsRepo`,
//!   `LogRepo`, `EventRepo`, `AlertRepo`, `NotifierRepo`, `SettingsRepo`, `VaultRepo`),
//!   `uniwrt-notify` (the notifiers' policy, formats and transports), `uniwrt-topo`, `uniwrt-clients`, `uniwrt-metrics`, `uniwrt-syslog` (what the
//!   receiver hands on, the annotations), tokio, tracing,
//!   `serde_json` (the history's frames).
//! - **Tests:** unit tests next to the pure parts (intervals and the poller, status, roams);
//!   `actor/tests.rs` drives actors over a scripted link (`testing.rs`) with tokio's paused
//!   clock, answering collects with the hwsim lab's capture; `topology_tests.rs` builds the
//!   topology of the lab's captures (`cargo nextest run -p uniwrt-engine`).

mod actor;
mod alerts;
mod bus;
mod clients;
mod config;
mod engine;
mod events;
mod fleet;
mod history;
mod interest;
mod live;
mod logs;
mod metrics;
mod notify;
mod roam;
mod schedule;
mod snapshots;
mod status;
#[cfg(test)]
mod testing;
mod topology;

pub use alerts::{AlertEngine, Alerts, AlertsConfig, AlertsError};
pub use bus::Event;
pub use clients::{ClientRecorder, RecorderConfig};
pub use config::EngineConfig;
pub use engine::Engine;
pub use events::{EventRecorder, EventsConfig};
pub use fleet::Fleet;
pub use history::{HistoryConfig, TopologyHistory, replay};
pub use interest::{Interest, Interests};
pub use live::NodeView;
pub use logs::{LogRecorder, LogsConfig};
pub use metrics::{MetricsConfig, MetricsRecorder};
pub use notify::{Notifications, Notify, NotifyConfig, NotifyStopped};
pub use schedule::Intervals;
pub use snapshots::Snapshots;
pub use topology::Identity;

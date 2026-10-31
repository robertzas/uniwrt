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
//!   kept; [`HistoryConfig`]) and [`replay`], which turns its frames back into a topology.
//! - **Actors** (ARCHITECTURE.md, Process structure): one task per node, restarted with backoff
//!   if it panics. After each hello it subscribes to the agent's topics, collects every set,
//!   and schedules ticks on the node's plan (15 s base, 2 s focused; D-037); an agent without an
//!   event loop, or one that refuses ticks, is polled with `collect` instead. A pushed event is
//!   announced at once and collects the set it touches again after a short delay. Roams are
//!   inferred across nodes from station events ([`uniwrt_model::event::Roam`]).
//! - **Topology** (`uniwrt_topo`): built again from the live state 1 s after a node's facts or
//!   status change (the changes of that second share one build), against the previous build for
//!   traffic rates, and announced as [`Event::Topology`] when it changed.
//! - **Status** (`model::status`): offline while the link is not up (connecting, unreachable,
//!   quarantined), degraded while the agent is not running or its telemetry is partial (failed
//!   sets or requests) or late (none for three of its fastest intervals, at least 30 s: the
//!   agent is then asked for its pushes again), else online.
//! - **Invariants:** an actor sends its agent one request at a time. Facts are replaced, never
//!   mutated: readers hold `Arc`s, and [`Event::Facts`] goes out only when a part changed. A
//!   removed node's actor has stopped before [`Engine::remove`] returns. Timers use tokio's
//!   clock and timestamps the injected `Clock`.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-node` (the link, the ops and the
//!   parsers), `uniwrt-store` (`FactsRepo`, `TopologyRepo`), `uniwrt-topo`, tokio, tracing,
//!   `serde_json` (the history's frames).
//! - **Tests:** unit tests next to the pure parts (intervals and the poller, status, roams);
//!   `actor/tests.rs` drives actors over a scripted link (`testing.rs`) with tokio's paused
//!   clock, answering collects with the hwsim lab's capture; `topology_tests.rs` builds the
//!   topology of the lab's captures (`cargo nextest run -p uniwrt-engine`).

mod actor;
mod bus;
mod config;
mod engine;
mod fleet;
mod history;
mod interest;
mod live;
mod roam;
mod schedule;
mod snapshots;
mod status;
#[cfg(test)]
mod testing;
mod topology;

pub use bus::Event;
pub use config::EngineConfig;
pub use engine::Engine;
pub use fleet::Fleet;
pub use history::{HistoryConfig, TopologyHistory, replay};
pub use interest::{Interest, Interests};
pub use live::NodeView;
pub use schedule::Intervals;
pub use snapshots::Snapshots;
pub use topology::Identity;

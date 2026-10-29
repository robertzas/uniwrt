//! The engine: an actor per node over its `NodeLink`, the live state of the fleet, the event
//! bus, telemetry intervals that follow interest, snapshots, and each node's status.
//!
//! - **API:** [`Engine`] ([`add`](Engine::add) a node with its link, [`remove`](Engine::remove)
//!   it, read it with [`node`](Engine::node) and [`all`](Engine::all) as [`NodeView`]s, take its
//!   [`link`](Engine::link) for requests of one's own, [`subscribe`](Engine::subscribe) to the
//!   bus's [`Event`]s); [`Interest`] guards from [`Engine::focus`] (fast intervals while the UI
//!   has the node open) and [`Engine::follow_logs`] (the node's log, as [`Event::Log`]);
//!   [`EngineConfig`] with the [`Intervals`] of each collect set; [`Snapshots`] (the latest facts
//!   of each node, saved through `FactsRepo`).
//! - **Actors** (ARCHITECTURE.md, Process structure): one task per node, restarted with backoff
//!   if it panics. After each hello it subscribes to the agent's topics, collects every set,
//!   and schedules ticks on the node's plan (15 s base, 2 s focused; D-037); an agent without an
//!   event loop, or one that refuses ticks, is polled with `collect` instead. A pushed event is
//!   announced at once and collects the set it touches again after a short delay. Roams are
//!   inferred across nodes from station events ([`uniwrt_model::event::Roam`]).
//! - **Status** (`model::status`): offline while the link is not up (connecting, unreachable,
//!   quarantined), degraded while the agent is not running or its telemetry is partial (failed
//!   sets or requests) or late (none for three of its fastest intervals, at least 30 s: the
//!   agent is then asked for its pushes again), else online.
//! - **Invariants:** an actor sends its agent one request at a time. Facts are replaced, never
//!   mutated: readers hold `Arc`s, and [`Event::Facts`] goes out only when a part changed. A
//!   removed node's actor has stopped before [`Engine::remove`] returns. Timers use tokio's
//!   clock and timestamps the injected `Clock`.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-node` (the link, the ops and the
//!   parsers), `uniwrt-store` (`FactsRepo`), tokio, tracing.
//! - **Tests:** unit tests next to the pure parts (intervals and the poller, status, roams);
//!   `actor/tests.rs` drives actors over a scripted link (`testing.rs`) with tokio's paused
//!   clock, answering collects with the hwsim lab's capture (`cargo nextest run -p
//!   uniwrt-engine`).

mod actor;
mod bus;
mod config;
mod engine;
mod interest;
mod live;
mod roam;
mod schedule;
mod snapshots;
mod status;
#[cfg(test)]
mod testing;

pub use bus::Event;
pub use config::EngineConfig;
pub use engine::Engine;
pub use interest::Interest;
pub use live::NodeView;
pub use schedule::Intervals;
pub use snapshots::Snapshots;

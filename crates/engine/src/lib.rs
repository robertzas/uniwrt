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
//!   dnsmasq's query log goes to the DNS query statistics instead ([`DnsStatsConfig`]: queries
//!   paired with their answers, stored through `DnsQueryRepo`, 24 h of queries and 30 days of
//!   hourly counts per client; D-133);
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
//!   [`Notify`] is the API's handle on it (reload, test);
//!   [`RfScanner`] ([`RfConfig`]) scans each radio's neighbors through its node's link (every
//!   few hours while the radio is idle, and when an admin asks; one scan at a time), stores the
//!   scans through `RfRepo` (30 days) and builds the RF report (`uniwrt_rf`) from the facts, the
//!   latest scans and the radar history, announced as [`Event::Rf`]; [`Rf`] is the API's handle
//!   on it (view, scan, settings); [`DriftWatch`] ([`DriftConfig`]) compares the agents' hashes
//!   of `/etc/config` with what applies confirmed (`DriftRepo`), reads and diffs a package whose
//!   hash moved, stores its drift and announces [`Event::Drift`]; [`Drifts`] is the API's
//!   handle on it (state, acknowledge, mute); [`WlanSchedules`] ([`SchedulesConfig`]) keeps
//!   the WLANs to their schedules (the saved site's), turning managed nodes' BSSes off and on
//!   through the agent's `bss` op, and announces the turns and refusals as events (D-083);
//!   [`WanMonitor`] ([`WanMonitorConfig`]) probes the internet from the gateway every 30 s
//!   (`uniwrt_node::probe`), stores each target's round as `probe:<key>` metrics and raises
//!   `wan_outage` and `wan_restored`; [`WanMonitorHandle`] is the API's handle on it (D-144);
//!   [`LedSchedule`] ([`LedScheduleConfig`]) keeps the managed nodes' LEDs dark while the
//!   site's LEDs schedule is on, through the agent's `leds` op (D-138);
//! - [`TailscaleSync`] ([`TailscaleConfig`]) keeps the gateway's Tailscale daemon to the site
//!   (prefs, up or down, a login with the auth key) through the agent's `tailscale` op (D-140);
//!   [`ClientBans`] ([`BansConfig`]) keeps blocked clients ([`Engine::block_client`]) and kicked
//!   ones off the managed nodes' access points through the agent's `ban` op, renewing hostapd's
//!   bans while it runs; [`Bans`] is the API's handle on it ([`Bans::kick`]; D-084);
//!   [`wake`] wakes a client from the gateway with `etherwake`, installing it there when the
//!   gateway lacks it and may be changed ([`Woken`], [`WakeError`]; D-137), through
//!   [`Fleet::exec`], which runs a command over a watched node's link;
//!   [`BlocklistSync`] ([`BlocklistConfig`]) refreshes DNS filtering's lists (`uniwrt_dns`),
//!   compiles them for the gateway's applied revision and pushes the file when the gateway's
//!   copy differs; [`Blocklist`] is the API's handle on it (its state, [`Blocklist::refresh`];
//!   D-133);
//!   [`NeighborReports`] ([`NeighborsConfig`]) gives the managed nodes' 802.11k access points
//!   their neighbor lists through the agent's `rrm` op, ranked by the RF report's matrix;
//!   [`ClientSteering`] ([`SteeringConfig`]) decides band steering and roam suggestions on the
//!   site's steering profile and sends them through the agent's `btm` op (D-093);
//!   [`Diagnostics`] ([`DiagConfig`]) runs the diagnostics toolkit's tools on nodes over exec
//!   streams ([`Fleet::stream`]), keeps each run's output in memory and announces it as
//!   [`DiagUpdate`]s; [`Watch`] keeps a run from being stopped as unwatched (D-141);
//!   [`Diagnostics::capture`] starts a packet capture, a [`Capture`] whose `.pcap` goes to one
//!   reader, cut at its size and time caps (D-142).
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
//!   `LogRepo`, `DnsQueryRepo`, `EventRepo`, `AlertRepo`, `NotifierRepo`, `RfRepo`, `SettingsRepo`, `VaultRepo`,
//!   `DriftRepo`, `RevisionRepo`), `uniwrt-uci` (drift's semantic diff), `sha2` (the hash of a file read),
//!   `uniwrt-notify` (the notifiers' policy, formats and transports), `uniwrt-rf` (the RF
//!   report), `uniwrt-topo`, `uniwrt-clients`, `uniwrt-metrics`, `uniwrt-syslog` (what the
//!   receiver hands on, the annotations), tokio, tracing,
//!   `serde_json` (the history's frames).
//! - **Tests:** unit tests next to the pure parts (intervals and the poller, status, roams);
//!   `actor/tests.rs` drives actors over a scripted link (`testing.rs`) with tokio's paused
//!   clock, answering collects with the hwsim lab's capture; `topology_tests.rs` builds the
//!   topology of the lab's captures (`cargo nextest run -p uniwrt-engine`).

mod actor;
mod alerts;
mod bans;
mod blocklist;
mod bus;
mod clients;
mod config;
mod diag;
mod drift;
mod engine;
mod events;
mod feeds;
mod fleet;
mod history;
mod interest;
mod leds;
mod live;
mod logs;
mod metrics;
mod neighbors;
mod notify;
mod rf;
mod roam;
mod schedule;
mod snapshots;
mod status;
mod steering;
mod tailscale;
#[cfg(test)]
mod testing;
mod topology;
mod wake;
mod wan_monitor;
mod wlans;

pub use alerts::{AlertEngine, Alerts, AlertsConfig, AlertsError};
pub use bans::{Bans, BansConfig, BansError, ClientBans, KickFailure, Kicked, KickedFrom};
pub use blocklist::{Blocklist, BlocklistConfig, BlocklistError, BlocklistSync};
pub use bus::Event;
pub use clients::{ClientRecorder, RecorderConfig};
pub use config::EngineConfig;
pub use diag::{
    Capture, CaptureEnd, CaptureError, DiagConfig, DiagError, DiagUpdate, Diagnostics, RunHosts,
    Starter, Watch, produced_by,
};
pub use drift::{DriftConfig, DriftError, DriftWatch, Drifts};
pub use engine::Engine;
pub use events::{EventRecorder, EventsConfig};
pub use feeds::{FeedMirror, FeedMirrorConfig, FeedMirrorError, FeedMirrorSync, plan as feed_plan};
pub use fleet::{Exec, Fleet, Stream, TailscaleCall, TerminalStart};
pub use history::{HistoryConfig, TopologyHistory, replay};
pub use interest::{Interest, Interests};
pub use leds::{LedSchedule, LedScheduleConfig};
pub use live::NodeView;
pub use logs::{DnsStatsConfig, LogRecorder, LogsConfig};
pub use metrics::{MetricsConfig, MetricsRecorder};
pub use neighbors::{NeighborReports, NeighborsConfig, OwnReports};
pub use notify::{Notifications, Notify, NotifyConfig, NotifyStopped};
pub use rf::{Rf, RfConfig, RfError, RfScanner};
pub use schedule::Intervals;
pub use snapshots::Snapshots;
pub use steering::{ClientSteering, SteeringConfig};
pub use tailscale::{TAILSCALE_TIMEOUT, TailscaleConfig, TailscaleSync, step as tailscale_step};
pub use topology::Identity;
pub use wake::{WakeError, Woken, wake};
pub use wan_monitor::{
    OUTAGE_CONDITION, WanMonitor, WanMonitorConfig, WanMonitorHandle, WanMonitorState,
};
pub use wlans::{SchedulesConfig, WlanSchedules};

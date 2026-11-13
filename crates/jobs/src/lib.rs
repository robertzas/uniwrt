//! Jobs (ARCHITECTURE.md, Job runner): what the controller does on nodes step by step, each in
//! its own task, recorded as history. Adoption first (features/adoption.md, Path A).
//!
//! - **API:** [`Jobs::start`] from a [`JobsConfig`] ([`Jobs::start_with`] and an [`ApplyTiming`] in tests) (the store, the vault, the clock, the
//!   controller key, a [`Dial`] to reach nodes and a [`Watch`] to start watching them);
//!   [`Jobs::probe`] reads a node's host key for the user to confirm; [`Jobs::adopt`] starts
//!   adopting a node ([`AdoptRequest`]) and returns the job as it starts (`model::job::Job`);
//!   [`Jobs::manage`] switches a node's management on or off (D-069); [`Jobs::wpad`] swaps a
//!   managed node's basic wpad for the full one (D-089); [`Jobs::set_role`] changes a node's
//!   role (D-100); [`Jobs::plan`] plans
//!   applying the site's latest revision and [`Jobs::apply`] applies it (D-070);
//!   [`Jobs::test_wan`] tries a WAN on the gateway and rolls it back ([`WanTest`], D-075);
//!   [`Jobs::speed_test`] measures the line from the gateway ([`SpeedTestRequest`], D-076);
//!   [`Jobs::import`] drafts a brownfield import of the adopted nodes ([`ImportDraft`], D-072);
//!   [`Jobs::discover`] reports the unadopted OpenWrt devices the adopted nodes see;
//!   [`Jobs::find_gateways`] looks for the factory-default router to set up at first run
//!   ([`bootstrap`], D-080);
//!   [`Jobs::job`], [`Jobs::jobs`] and [`Jobs::subscribe`] follow jobs; [`Jobs::shutdown`] stops
//!   them. [`SshDial`] reaches nodes over SSH; [`Remote`] is one login. Errors are [`Error`];
//!   why a job failed is its `JobError`, a code the UI can act on plus a message.
//! - **Adoption** steps: connect (with the root password, or the controller's key when the node
//!   trusts it, or without a password on factory defaults) → check (the release, then the
//!   agent's facts: memory, `/tmp`, identity, not adopted yet) → install the key → log in with
//!   it → set a random root password where root had none (sealed in the vault first) →
//!   inventory (packages) → backup
//!   (`sysupgrade -b`, sealed) → record (node, pin, facts, inventory and backup in one
//!   transaction; the password in the vault if asked) → watch. At most 4 run at once; more wait.
//! - **Management** steps: connect (the controller's key, the pinned host key) → install the
//!   transaction tool and the guard unless that version is there (or remove them) → record the
//!   mode. An open transaction on the node fails the job before anything changes.
//! - **wpad swap** (`wpad/`, D-089): connect → check (an idle transaction tool, a hostapd
//!   without 802.11v, the wpad installed and its full counterpart, free flash and `/tmp`) →
//!   fetch (both wpads and `hostapd-common` onto the node, nothing changed) → swap (one offline
//!   package step that runs on the node by itself and puts the old wpad back when neither is
//!   left; the job logs in again until it reads the result) → verify (the agent finds 802.11v).
//! - **Apply** steps (`apply/`): connect (every managed node) → plan (read each node's packages,
//!   services, facts and DNS at once; render; diff; any refused node stops it) → install (the
//!   feed packages a configuration needs and a node lacks, D-076) → stage → apply
//!   (leaves → gateway, dead-man timers armed) → verify (all at once, over new logins at the
//!   address each moves to, health against the baseline) → confirm → settle and wan (the
//!   gateway's later transactions, D-081) → backhaul (the cables between nodes probed, then
//!   exclusive or trunks again, D-097) → record (the revision each node holds; renumbered nodes
//!   moved and watched again). A failure before a confirm rolls
//!   every applied node back; a later lost confirm is a partial apply. It holds every managed
//!   node's key.
//! - **WAN test** (`wan_test/`): connect (the gateway) → plan (the latest revision with the WAN
//!   tried, rendered; only the WAN's sections of its own `network` change) → stage → apply →
//!   the connectivity checks live (link, address, gateway, DNS, HTTPS, IPv6; a second trial
//!   with a 6rd fallback when DHCPv6 delegates no prefix) → rollback, always. It fails
//!   (`no_internet`, with the hint) when IPv4 doesn't reach the internet.
//! - **Speed test** (`speed_test/`): connect (the gateway) → idle (round trips) → download →
//!   upload (four HTTPS streams for 12 s each, pinging; SQM stopped for them with
//!   `bypass_sqm`) → record (the history). Rates leave the warm-up out; latency under load is
//!   the 90th percentile, graded for bufferbloat.
//! - **Import** (`import.rs`, no job: the caller waits): log in to every adopted node (observe
//!   mode too) → read the packages render writes and the facts → `uniwrt_import` on the latest
//!   revision → seal the secrets found (keeping a reference whose vault value is the same) → the
//!   draft. Read-only; a node with an open transaction, or not reached, is left unread.
//! - **Discovery:** the devices in the adopted nodes' DHCP leases, IPv4 neighbors (not on WAN)
//!   and batman-adv meshes, less the adopted nodes, probed on port 22 (the banner, then the
//!   host key of dropbear servers and of hosts named `OpenWrt` or `uniwrt-*`; never a login),
//!   4 at a time, each result held 10 minutes. Reported: dropbear servers, those names, and mesh
//!   originators none of whose devices is reported (with no address).
//! - **Invariants:** nothing on a node changes before every check passed, and in observe mode
//!   only `authorized_keys` changes, and root's password where it had none (directive 8); the agent runs from `/tmp`. The node must
//!   present the host key the user confirmed. One job at a time per target. Each change of a
//!   step is saved before it is announced. Jobs do not resume (D-042): a stop fails the running
//!   ones, at shutdown or else at the next start. Passwords are zeroized and never logged or
//!   kept in jobs.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-node` (SSH, the agent, the facts
//!   parsers, the transaction tool), `uniwrt-render`, `uniwrt-import`, `uniwrt-uci`, `uniwrt-topo` (the apply's
//!   order), `uniwrt-store` (jobs, adoption, the vault, applied revisions), tokio,
//!   `futures-util`, `zeroize`.
//! - **Tests:** unit tests of the parsers and checks; `discovery/tests.rs` discovers over
//!   scripted facts and a scripted [`Dial`]; `adopt/tests.rs` and `manage_tests.rs` run
//!   jobs over the scripted node of `testing.rs` and a temporary store; `apply/tests/` applies
//!   over a scripted fleet running `uniwrt_sim::txn`, which `import_tests.rs` imports; `tests/agent.rs` adopts OpenWrt in a container
//!   (`just test-agent`).

mod adopt;
mod apply;
pub mod bootstrap;
mod discovery;
mod errors;
mod import;
mod manage;
#[cfg(test)]
mod manage_tests;
mod progress;
mod remote;
mod role;
mod runner;
mod speed_test;
#[cfg(test)]
mod testing;
mod wan_test;
mod wpad;
mod ztp;

pub use adopt::{AdoptRequest, ZtpClaim};
pub use apply::ApplyTiming;
pub use import::{ImportDraft, ImportScope};
pub use remote::{BoxFuture, Dial, Remote, SshDial};
pub use runner::{Jobs, JobsConfig, Watch};
pub use speed_test::SpeedTestRequest;
pub use wan_test::WanTest;
pub use ztp::ZtpScript;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Nodes cannot be adopted here: there is no dial.
    #[error("this controller cannot reach nodes")]
    Unavailable,
    #[error("a job is running for {target} already")]
    Busy { target: String },
    #[error("there is no node {0}")]
    NoNode(uniwrt_core::NodeId),
    #[error("the site has no revision yet")]
    NoRevision,
    #[error("no gateway is adopted")]
    NoGateway,
    /// The node is in observe mode: nothing may change on it.
    #[error("node {0} is in observe mode")]
    Observed(uniwrt_core::NodeId),
    /// The revision to apply is no longer the latest.
    #[error("revision {revision} is not the latest: {head} is")]
    Stale {
        revision: uniwrt_core::RevisionId,
        head: uniwrt_core::RevisionId,
    },
    #[error("{0}")]
    Failed(String),
    /// The change breaks a rule of the site (its one gateway): why, for the user.
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Node(#[from] uniwrt_node::Error),
    #[error(transparent)]
    Store(#[from] uniwrt_store::Error),
}

//! Jobs (ARCHITECTURE.md, Job runner): what the controller does on nodes step by step, each in
//! its own task, recorded as history. Adoption first (features/adoption.md, Path A).
//!
//! - **API:** [`Jobs::start`] from a [`JobsConfig`] (the store, the vault, the clock, the
//!   controller key, a [`Dial`] to reach nodes and a [`Watch`] to start watching them);
//!   [`Jobs::probe`] reads a node's host key for the user to confirm; [`Jobs::adopt`] starts
//!   adopting a node ([`AdoptRequest`]) and returns the job as it starts (`model::job::Job`);
//!   [`Jobs::discover`] reports the unadopted OpenWrt devices the adopted nodes see;
//!   [`Jobs::job`], [`Jobs::jobs`] and [`Jobs::subscribe`] follow jobs; [`Jobs::shutdown`] stops
//!   them. [`SshDial`] reaches nodes over SSH; [`Remote`] is one login. Errors are [`Error`];
//!   why a job failed is its `JobError`, a code the UI can act on plus a message.
//! - **Adoption** steps: connect (with the root password, or the controller's key when the node
//!   trusts it) → check (the release, then the agent's facts: memory, `/tmp`, identity, not
//!   adopted yet) → install the key → log in with it → inventory (packages) → backup
//!   (`sysupgrade -b`, sealed) → record (node, pin, facts, inventory and backup in one
//!   transaction; the password in the vault if asked) → watch. At most 4 run at once; more wait.
//! - **Discovery:** the devices in the adopted nodes' DHCP leases, IPv4 neighbors (not on WAN)
//!   and batman-adv meshes, less the adopted nodes, probed on port 22 (the banner, then the
//!   host key of dropbear servers and of hosts named `OpenWrt` or `uniwrt-*`; never a login),
//!   4 at a time, each result held 10 minutes. Reported: dropbear servers, those names, and mesh
//!   originators none of whose devices is reported (with no address).
//! - **Invariants:** nothing on a node changes before every check passed, and in observe mode
//!   only `authorized_keys` changes (directive 8); the agent runs from `/tmp`. The node must
//!   present the host key the user confirmed. One job at a time per target. Each change of a
//!   step is saved before it is announced. Jobs do not resume (D-042): a stop fails the running
//!   ones, at shutdown or else at the next start. Passwords are zeroized and never logged or
//!   kept in jobs.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-node` (SSH, the agent, the facts
//!   parsers), `uniwrt-store` (jobs, adoption, the vault), tokio, `zeroize`.
//! - **Tests:** unit tests of the parsers and checks; `discovery/tests.rs` discovers over
//!   scripted facts and a scripted [`Dial`]; `adopt/tests.rs` runs adoptions over a
//!   scripted [`Dial`] and a temporary store; `tests/agent.rs` adopts OpenWrt in a container
//!   (`just test-agent`).

mod adopt;
mod discovery;
mod progress;
mod remote;
mod runner;

pub use adopt::AdoptRequest;
pub use remote::{BoxFuture, Dial, Remote, SshDial};
pub use runner::{Jobs, JobsConfig, Watch};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Nodes cannot be adopted here: demo mode simulates its fleet.
    #[error("this controller cannot reach nodes (demo mode)")]
    Unavailable,
    #[error("a job is running for {target} already")]
    Busy { target: String },
    #[error(transparent)]
    Node(#[from] uniwrt_node::Error),
    #[error(transparent)]
    Store(#[from] uniwrt_store::Error),
}

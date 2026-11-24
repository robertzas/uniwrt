//! The node transport: SSH to OpenWrt nodes (russh), the node agent (its protocol, its embedded
//! bundle and the parsers of what it collects), and the [`NodeLink`] trait through which the
//! engine talks to a node.
//!
//! - **API:** [`ControllerKey`] (the controller's ed25519 identity, kept in the vault as OpenSSH
//!   text); [`HostKey`] and [`probe`] (trust on first use: read a node's host key so the user can
//!   confirm its fingerprint before it is pinned); [`banner`] (an SSH server's version line, read
//!   without a handshake, for discovery, directly or through a jump session); [`Session`] (one authenticated connection,
//!   direct or through a jump session over `direct-tcpip`) with [`Session::exec`],
//!   [`Session::stream`], [`Session::terminal`] (a login shell on a pseudo-terminal, D-141) and
//!   [`Session::upload`]; [`Agent`] (the RPC multiplexer over a running
//!   agent's stdio, see [`proto`]); [`proto::ops`] (the typed ops, sent with
//!   [`NodeLink::request`]); [`Bundle`] (`node-agent/`, installed by hash into
//!   `/tmp/uniwrt/agent`); [`facts`] (collect sets parsed into `model::facts`); [`events`] (the
//!   agent's pushes, from [`NodeLink::events`], parsed into ticks, `model::event`s and log
//!   entries); [`SshLink`] (a
//!   supervised connection that reconnects with jittered backoff, installs and restarts the
//!   agent, and implements [`NodeLink`]); [`txn`] (the transaction tool's commands, the
//!   [`Footprint`](txn::Footprint) that management installs, and [`txn::current`], the reads of
//!   a node's packages and services); [`archive`] (the `sysupgrade -b` tarball read and written
//!   in memory, and its content digest, D-126); [`pkg`] (opkg's and apk's commands and the installed
//!   packages' parser, D-100); [`probe`] (the WAN quality monitor's round, `probe.uc` over exec,
//!   and its traceroute; D-144); [`wpad`] (the commands that swap a node's basic wpad for
//!   the full one, D-089); [`wol`] (Wake-on-LAN from the gateway: `etherwake` and the devices
//!   to send on, D-137); [`decommission`] (the detached wipe that takes a node out of the
//!   site: the mesh left, the footprint and the controller's key removed, `firstboot`
//!   optionally, D-131); [`diag`] (the diagnostics toolkit's commands and their time limits,
//!   D-141; the tables' parsers, a packet capture's command and size cut, and iperf3's commands,
//!   D-142); [`iperf3`] (a native iperf3 server: the controller's end of a node ↔ controller
//!   throughput test, D-142); [`root_password`] (a random root password for a node
//!   without one, and the `passwd` that sets it); [`ztp`] (the first-boot script of
//!   zero-touch images, D-102); [`adoption_port`] (the gateway's segment and the device found
//!   on it: the reads and their parsers, D-106); [`Error`]. A [`Credential::Password`] that is
//!   empty logs in with SSH's `none` method first, as factory-default OpenWrt lets root in.
//! - **Invariants:** every connection checks the host key against its pin; a mismatch is
//!   [`Error::HostKeyMismatch`] and quarantines the link ([`LinkState::Quarantined`]), never a
//!   warning. Commands are argv arrays quoted for the remote shell ([`shell::command_line`]),
//!   never interpolated. Each channel is drained by its own task, so a slow consumer cannot
//!   stall the connection: a stream with more unread output than its budget ends with
//!   [`Error::Overflow`]. Operations fail fast while a node is down. A link runs only the agent
//!   whose hash its hello reports. No RSA keys and no SSH certificates (D-032).
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (the host-key pin, the facts), russh with the
//!   ring provider, tokio, `serde_json` (raw values: answers are parsed once, by whoever wants
//!   them), `zeroize`, `data-encoding` (base64 in agent output).
//! - **Tests:** unit tests next to the pure parts (quoting through the local `sh`, backoff, line
//!   framing, the envelope, the multiplexer over an in-memory pipe, the bundle's archive through
//!   the host's `tar`, the parsers against `fixtures/collect/` and `fixtures/events/`); `tests/` runs sessions and links
//!   against an in-process russh server on loopback. Ignored unless run by their scripts:
//!   `tests/dropbear.rs`, `tests/capture.rs` and `tests/iperf3.rs` (`just test-dropbear`) against
//!   dropbear and iperf3 in OpenWrt rootfs containers, and
//!   `tests/agent.rs` (`just test-agent`) against the agent on OpenWrt booted with procd, and
//!   `tests/wifi.rs` (`just test-wifi`) against the hwsim lab's radios and mesh.

pub mod adoption_port;
mod agent;
pub mod archive;
mod backoff;
mod bundle;
pub mod decommission;
pub mod diag;
mod error;
pub mod events;
pub mod exec;
pub mod facts;
mod host_key;
pub mod iperf3;
mod key;
mod lines;
mod link;
pub mod pkg;
pub mod probe;
pub mod proto;
pub mod root_password;
pub mod shell;
mod ssh;
mod supervisor;
pub mod txn;
pub mod wol;
pub mod wpad;
pub mod ztp;

pub use agent::{Agent, HELLO_TIMEOUT};
pub use bundle::Bundle;
pub use error::{Error, Result};
pub use exec::{ExecOutput, ExecRequest, ExecStream, ExitStatus, Output, PtySize, Terminal};
pub use host_key::HostKey;
pub use key::{AUTHORIZED_KEY_COMMENT, ControllerKey};
pub use link::{AgentState, LinkState, NodeLink};
pub use proto::{AgentEvent, Hello};
pub use ssh::{Credential, Session, SshConfig, Target, banner, probe};
pub use supervisor::{AgentLaunch, LinkConfig, RetryConfig, SshLink};

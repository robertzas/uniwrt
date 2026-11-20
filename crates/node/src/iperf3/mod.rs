//! A native iperf3 server (D-142): the far end of a node ↔ controller throughput test, so that
//! the controller needs no iperf3 of its own. It speaks iperf3's TCP control protocol and its
//! TCP data streams as iperf 3.x clients do (checked on the wire against iperf 3.22, OpenWrt
//! 24.10's and 25.12's): the client's cookie, the parameters as length-prefixed JSON, the
//! streams (each opened with the cookie; ids 1, 3, 4, … as iperf3 numbers them), the test, and
//! the results exchanged as JSON. Both directions and both at once: the server receives (the
//! client's default), sends (`-R`) or does both (`--bidir`, the first half of the streams
//! received). UDP, SCTP and tests bounded by bytes or blocks are refused.
//!
//! [`serve`] takes one test from the addresses [`ServerConfig::allowed`] names, on a listener
//! bound for it, and answers what each stream carried; connections from other addresses are
//! closed unread, and another client's cookie during the test is denied. Every step of the
//! handshake has a time limit, and the test's length is capped. Dropping the future ends the
//! test and closes every socket.

mod proto;
mod server;
#[cfg(test)]
mod tests;

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

pub use self::server::serve;

/// What [`serve`] takes.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The addresses whose connections are taken (IPv4-mapped IPv6 addresses match their IPv4
    /// address); others are closed at once.
    pub allowed: Vec<IpAddr>,
    /// The longest test, in seconds (the client's `-t`, omitted seconds excluded).
    pub max_seconds: u32,
    /// The most streams.
    pub max_streams: u32,
    /// How long each step of the handshake may take.
    pub handshake: Duration,
}

impl ServerConfig {
    /// Whether a connection from `ip` is taken.
    pub fn allows(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.allowed
            .iter()
            .any(|allowed| allowed.to_canonical() == ip)
    }
}

/// What a test carried.
#[derive(Debug, Clone, PartialEq)]
pub struct Iperf3Summary {
    /// The client's control connection.
    pub peer: SocketAddr,
    pub streams: Vec<StreamSummary>,
}

/// What one stream carried, as the server measured it (omitted seconds excluded).
#[derive(Debug, Clone, PartialEq)]
pub struct StreamSummary {
    /// iperf3's id of the stream.
    pub id: u32,
    /// The server sent on it (else it received).
    pub sent: bool,
    pub bytes: u64,
    pub seconds: f64,
}

impl Iperf3Summary {
    /// The bytes the server received and sent, in all.
    pub fn totals(&self) -> (u64, u64) {
        self.streams
            .iter()
            .fold((0, 0), |(received, sent), stream| {
                if stream.sent {
                    (received, sent + stream.bytes)
                } else {
                    (received + stream.bytes, sent)
                }
            })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Iperf3Error {
    #[error("{what}: {source}")]
    Io {
        what: &'static str,
        #[source]
        source: std::io::Error,
    },
    /// The client asked for what the server doesn't do, or broke the protocol.
    #[error("{0}")]
    Protocol(String),
    #[error("the client didn't {what} within {} s", .after.as_secs())]
    Timeout { what: &'static str, after: Duration },
    /// The client ended the test early, or went away.
    #[error("the client ended the test")]
    Terminated,
}

impl Iperf3Error {
    /// An IO error while doing `what`; the connection's end is the client going away.
    pub(crate) fn io(what: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| match source.kind() {
            std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe => Self::Terminated,
            _ => Self::Io { what, source },
        }
    }
}

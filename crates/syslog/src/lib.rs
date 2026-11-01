//! Syslog: the RFC 3164/5424 parser, the UDP and TCP receiver, and the annotations that explain
//! log entries (observability.md, Logs).
//!
//! - **API:** [`parse`] turns one message into a [`Message`] (its entry, and the host it names);
//!   [`Framer`] splits a TCP stream into messages (RFC 6587: octet-counted, or ended by a line
//!   feed or NUL). [`Receiver::bind`] listens on UDP and TCP ([`ReceiverConfig`], port 5514 by
//!   default) and hands each message on as a [`Received`], with the sender's address and when
//!   it arrived; it also says when a TCP sender disconnects. [`Patterns`] annotates entries from
//!   the embedded `data/log-patterns.toml` ([`Patterns::embedded`]).
//! - **Invariants:** the parser is lenient and never fails on input but an empty message: what
//!   does not parse becomes text. Messages are cut at [`MAX_MESSAGE`] bytes. The receiver never
//!   blocks a sender: each sender has a rate (token bucket), a full queue drops messages, and
//!   both count what they drop. At most [`ReceiverConfig::max_connections`] TCP connections
//!   at once. Timestamps come from the injected `Clock`.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (the entry and its severity), tokio (net),
//!   `time` (RFC 3339), `regex` and `toml` (the patterns), tracing.
//! - **Tests:** unit tests for the parser, the framer (proptest: chunking never changes the
//!   frames; any input parses without panicking) and the patterns (each with a real example
//!   line); `receiver.rs` sends over loopback (`cargo nextest run -p uniwrt-syslog`).

mod annotate;
mod frame;
mod parse;
mod receiver;

pub use annotate::{PatternError, Patterns};
pub use frame::Framer;
pub use parse::{MAX_MESSAGE, Message, parse};
pub use receiver::{Received, Receiver, ReceiverConfig};

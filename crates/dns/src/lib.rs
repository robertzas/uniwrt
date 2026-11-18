//! DNS filtering's lists (features/network.md, Blocklists; D-133).
//!
//! **Contract.** [`Blocklists`] keeps a copy of every list a site wants ([`wanted`]: the
//! catalog's malware lists, the user's own URLs, then the catalog's ads lists, most wanted
//! first) in a directory of the controller's: [`Blocklists::refresh`] asks each list's
//! [`Source`] again (a conditional GET over HTTP(S) with [`HttpSource`]), keeping the last good
//! copy when that fails and saying why ([`Blocklists::status`]); copies no longer wanted are
//! deleted. [`Blocklists::compile`] reads the copies into one dnsmasq `servers-file`
//! ([`compile()`]): a header line naming its SHA-256 and domain count
//! ([`uniwrt_model::blocklists::header`]), then `server=/<domain>/` lines (blocked, with every
//! subdomain) and `server=/<domain>/#` lines (allowed under a blocked parent), deduplicated,
//! sorted, and capped at what the gateway's memory holds
//! ([`uniwrt_model::blocklists::capacity`]), the user's own blocks first. Lists in hosts,
//! plain-domain, adblock (`||domain^`) or dnsmasq format are read ([`parse`]).
//!
//! When lists refresh and where the file goes are the engine's (the push, a2a1b2).

mod compile;
mod http;
mod lists;
pub mod parse;

pub use compile::{Compiled, compile};
pub use http::{BoxFuture, Fetched, HttpSource, MAX_LIST, Source, Validators};
pub use lists::{Blocklists, Wanted, wanted};

/// Why a list couldn't be had.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{url}: {why}")]
    Unreachable { url: String, why: String },
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url} is larger than {max_mib} MiB")]
    TooLarge { url: String, max_mib: usize },
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Storage(String),
}

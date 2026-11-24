//! The controller's package feed mirror (features/lifecycle.md, Offline packages; D-147).
//!
//! **Contract.** [`Mirror`] keeps OpenWrt feeds in a directory of the controller's, laid out as
//! their URLs are (`<root>/<host>/<path>/<file>`, [`Feed`]). [`Mirror::sync`] brings it to a
//! [`Plan`]: each feed set (one release, target and architecture) with its feeds and the
//! package names wanted of it; each feed's index is refreshed conditionally (a failure keeps
//! the copy kept), the wanted names and the names nodes fetched through the mirror are
//! resolved with their dependencies ([`closure`]; or every package, with `whole`), the
//! packages missing are downloaded from a [`Source`] ([`HttpSource`]: HTTP(S) with Mozilla's
//! roots, streamed to disk) and kept only once they match their index's size and hash
//! ([`verify`]), and every file no planned feed's index needs is deleted. [`Mirror::serve`]
//! answers a node's request: a planned feed's index (refreshed when over an hour old), or a
//! package its index lists, kept or fetched now (pull through). opkg's `Packages.gz` and
//! apk's binary `packages.adb` are read ([`index`]). [`Mirror::status`] says what is held.
//! A plan also names OpenWrt's official images for the upgrades on offer ([`Plan::images`]):
//! each is downloaded once and kept where its URL says, beside the feeds, only when its size
//! and SHA-256 are its release's; it isn't served, the upgrade takes it from disk
//! ([`Mirror::image`]). [`next::derive`] makes a release's feeds from those of the release a
//! node runs (the release, architecture and kernel modules' directory in their paths).
//!
//! **Invariants.** Every path is built from checked segments, so nothing is written or read
//! outside the root, and only planned feeds' indexes and listed packages are served or
//! fetched: the mirror is no open proxy. Files are written whole elsewhere first and renamed
//! into place; an index and its signature change together under the feed's lock. Signatures
//! aren't checked here: the node checks them, as with any feed.
//!
//! When the mirror syncs and what it plans are the engine's.

pub mod closure;
pub mod feed;
mod images;
pub mod index;
mod mirror;
pub mod next;
mod serve;
mod source;
mod state;
mod sync;
#[cfg(test)]
mod tests;
pub mod verify;

pub use feed::Feed;
pub use mirror::{Mirror, Plan, SetPlan};
pub use serve::{ServeError, Served};
pub use source::{BoxFuture, Fetched, HttpSource, Source, Validators};

/// Why a feed or a package couldn't be had, or kept.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{url}: {why}")]
    Unreachable { url: String, why: String },
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url} is larger than {max_mib} MiB")]
    TooLarge { url: String, max_mib: u64 },
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Storage(String),
}

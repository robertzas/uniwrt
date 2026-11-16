//! Firmware (features/adoption.md, Path B; features/lifecycle.md; D-022, D-104, D-121):
//! OpenWrt's catalog, zero-touch images and upgrade images built by ASU, OpenWrt's public
//! Attended Sysupgrade service.
//!
//! - **API:** [`Asu`] (one ASU service, `https://sysupgrade.openwrt.org` by default):
//!   [`Asu::releases`] (each supported branch's latest release, 24.10 or later, from ASU's
//!   `json/v1/overview.json`), [`Asu::profiles`] (a release's devices, from the download
//!   server ASU names: `releases/<release>/.overview.json`), [`Asu::build`] (an image of a
//!   device with the mesh essentials and the first-boot script, [`ImageRequest`]) and
//!   [`Asu::status`] (a build as it stands); for upgrades ([`Upgrade`]): [`Asu::offers`] (the
//!   latest of a node's branch and the next branch's, never skipping one, with the board's
//!   profile in each from the download server's `profiles.json`), [`Asu::upgrade_plan`] (the
//!   node's packages carried over with ASU's renames between the branches' branch-off
//!   revisions, checked against ASU's package indexes, the missing ones dropped and listed)
//!   and [`Asu::build_upgrade`] (that image, without a first-boot script), [`Asu::official_plan`]
//!   (the same plan with OpenWrt's own image, the packages installed after the upgrade); and
//!   [`Uploads`] (images admins upload, kept in a directory under their SHA-256 once their
//!   metadata names the boards they fit, or an x86 image's name its target; [`upload_fits`]);
//!   [`GitHub`] (D-122: a repo's releases, [`GitHub::releases`], read with an optional token
//!   and kept ten minutes; [`Releases::offers`], the newest [`MAX_OFFERS`] with an image for a
//!   [`Device`]: its profile or board in the asset's name, x86's subtarget; [`parse_repo`]);
//!   [`Error`]. With the `testing` feature, `testing::FakeAsu`, an ASU and download server on
//!   loopback, and `testing::FakeGitHub`, GitHub's releases API.
//! - **Invariants:** nothing secret goes to ASU (D-022): the request holds the device, the
//!   packages and the first-boot script, which holds only public data. Every value that
//!   becomes part of a URL (release, target, profile, build hash, image name) is checked
//!   against the catalog or a strict pattern first. The catalog is kept in memory (the
//!   releases an hour, a release's devices a day, four releases at most). Every package added
//!   is in the profile's own default set's terms (`-name` removes one), so ASU resolves them
//!   for the device's release. An upgrade never removes a profile's default: only the full
//!   wpad replaces the basic or mesh one, so a package list read short can't leave a driver
//!   or the package manager out (D-121). A target's devices and a release's indexes are kept
//!   a day (eight targets, four indexes). An upload is kept only once inspected: a file that
//!   names no board (and isn't x86's gzipped image) never reaches the directory; at most
//!   [`MAX_UPLOADS`] of at most [`MAX_UPLOAD_BYTES`]. A GitHub image is offered only with the
//!   SHA-256 GitHub records for it, and only when its name tells the board (or x86's
//!   subtarget) as a whole word; the token is sent to GitHub's API alone, marked sensitive.
//! - **Dependencies:** `uniwrt-model`, hyper, hyper-util, hyper-rustls (ring, Mozilla's roots),
//!   ring (SHA-256), `serde_json`, tokio (timeouts, files).

mod asu;
mod build;
mod catalog;
mod device;
mod github;
mod http;
mod image;
mod keep;
mod packages;
mod release;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;
mod uploads;

pub use asu::{Asu, ImageRequest, Upgrade};
pub use github::{Device, GitHub, MAX_OFFERS, Releases, parse_repo, valid_repo, valid_token};
pub use uploads::{MAX_UPLOAD_BYTES, MAX_UPLOADS, Receiving, Uploads, upload_fits, valid_sha256};

/// What went wrong talking to ASU, the download server or GitHub.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot reach {url}: {why}")]
    Unreachable { url: String, why: String },
    #[error("{url} answered {status}")]
    Status { url: String, status: u16 },
    /// An answer that isn't what it should be, or a value uniwrt can't send.
    #[error("{0}")]
    Invalid(String),
    /// An unknown release, device or build.
    #[error("{0}")]
    NotFound(String),
    /// ASU refused the request, in its words.
    #[error("ASU refused the build: {0}")]
    Refused(String),
    /// ASU is overloaded.
    #[error("ASU is busy, try again later: {0}")]
    Busy(String),
    /// GitHub refused, in words for people (its rate limit, another status).
    #[error("{0}")]
    GitHub(String),
    /// GitHub refused the token: it expired or was revoked.
    #[error("{0}")]
    Unauthorized(String),
    /// The uploaded images' directory failed.
    #[error("cannot keep the uploaded images: {0}")]
    Storage(String),
}

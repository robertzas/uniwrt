//! Firmware (features/adoption.md, Path B; D-022, D-104): OpenWrt's catalog and zero-touch
//! images built by ASU, OpenWrt's public Attended Sysupgrade service.
//!
//! - **API:** [`Asu`] (one ASU service, `https://sysupgrade.openwrt.org` by default):
//!   [`Asu::releases`] (each supported branch's latest release, 24.10 or later, from ASU's
//!   `json/v1/overview.json`), [`Asu::profiles`] (a release's devices, from the download
//!   server ASU names: `releases/<release>/.overview.json`), [`Asu::build`] (an image of a
//!   device with the mesh essentials and the first-boot script, [`ImageRequest`]) and
//!   [`Asu::status`] (a build as it stands); [`Error`]. With the `testing` feature,
//!   `testing::FakeAsu`, an ASU and download server on loopback.
//! - **Invariants:** nothing secret goes to ASU (D-022): the request holds the device, the
//!   packages and the first-boot script, which holds only public data. Every value that
//!   becomes part of a URL (release, target, profile, build hash, image name) is checked
//!   against the catalog or a strict pattern first. The catalog is kept in memory (the
//!   releases an hour, a release's devices a day, four releases at most). Every package added
//!   is in the profile's own default set's terms (`-name` removes one), so ASU resolves them
//!   for the device's release.
//! - **Dependencies:** `uniwrt-model`, hyper, hyper-util, hyper-rustls (ring, Mozilla's roots),
//!   `serde_json`, tokio (timeouts).

mod asu;
mod build;
mod catalog;
mod http;
mod packages;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use asu::{Asu, ImageRequest};

/// What went wrong talking to ASU or the download server.
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
}

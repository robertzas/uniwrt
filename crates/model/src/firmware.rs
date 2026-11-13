//! Firmware (features/adoption.md, Path B; D-022, D-104): OpenWrt's catalog of releases and
//! device profiles, and the zero-touch images the controller asks ASU (OpenWrt's public
//! Attended Sysupgrade service) to build. Nothing here is secret: ASU is a public service.

use serde::{Deserialize, Serialize};

/// The releases a zero-touch image can be built for: each supported branch's latest, newest
/// first (the first is the default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirmwareReleases {
    pub releases: Vec<String>,
}

/// A device in OpenWrt's catalog for one release: an image builder profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirmwareProfile {
    /// The profile, such as `glinet_gl-mt6000`.
    pub id: String,
    /// The target and subtarget, such as `mediatek/filogic`.
    pub target: String,
    /// The devices it builds for, for people: vendor, model and variant, such as
    /// `GL.iNet GL-MT6000` (several when one image fits several devices).
    pub titles: Vec<String>,
}

/// Where an image build stands at ASU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ImageBuildState {
    /// Waiting in ASU's queue.
    Queued,
    /// The image builder runs.
    Building,
    Done,
    Failed,
}

/// An image build at ASU, as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ImageBuild {
    /// ASU's request hash: the same request gets the same build, cached.
    pub hash: String,
    pub state: ImageBuildState,
    /// The builds ahead of it while [`ImageBuildState::Queued`].
    pub queue_position: Option<u32>,
    /// The image builder's step while [`ImageBuildState::Building`], as ASU names it
    /// (`container_setup`, `validate_manifest`, `building_image`, `signing_images`…).
    pub step: Option<String>,
    /// Why it failed, in ASU's words, with the end of the image builder's error output.
    pub error: Option<String>,
    /// The images once [`ImageBuildState::Done`]: the factory image (from a vendor's
    /// firmware) and the sysupgrade one (from OpenWrt) first.
    pub images: Vec<FirmwareImage>,
}

/// An image of a finished build, downloaded from ASU's store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FirmwareImage {
    pub name: String,
    /// What it is for: `factory`, `sysupgrade`, `kernel`, `combined`, `combined-efi`…
    pub kind: String,
    /// `squashfs`, `ext4`… when it has a root file system.
    pub filesystem: Option<String>,
    pub size_bytes: Option<u64>,
    /// Its SHA-256, in lowercase hex.
    pub sha256: String,
    /// Where to download it: ASU's store, over HTTPS.
    pub url: String,
}

/// An image build just asked for: the packages added to the profile's defaults, and how the
/// build stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ImageBuildStarted {
    /// The packages added to (or, with a leading `-`, removed from) the profile's defaults.
    pub packages: Vec<String>,
    pub build: ImageBuild,
}

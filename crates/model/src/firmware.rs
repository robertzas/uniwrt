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

/// Every adopted node's firmware with the upgrades on offer (M7.1a1, D-121).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirmwareCatalog {
    pub nodes: Vec<NodeFirmware>,
    /// The images admins uploaded (M7.1a2, D-122), newest first.
    pub uploads: Vec<FirmwareUpload>,
    /// Why the catalog couldn't be read (ASU unreachable, or turned off): no offers then.
    pub error: Option<String>,
}

/// A node's firmware as the catalog sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NodeFirmware {
    pub node: uniwrt_core::NodeId,
    /// `OpenWrt`, or a derivative's name.
    pub distribution: String,
    /// The release it runs, such as `24.10.8`.
    pub version: String,
    /// Its source revision, such as `r29233-443ec4032a`.
    pub revision: String,
    /// The target and subtarget, such as `mediatek/filogic`.
    pub target: String,
    /// The board images target, such as `glinet,gl-mt6000`.
    pub board: String,
    /// Newest first: the current branch's latest release, then the next branch's.
    pub offers: Vec<FirmwareOffer>,
    /// Why nothing is offered, when nothing is and the node isn't simply up to date.
    pub note: Option<String>,
    /// The uploaded images made for its board (by SHA-256), newest first.
    pub uploads: Vec<String>,
}

/// A release a node can upgrade to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirmwareOffer {
    pub release: String,
    pub kind: OfferKind,
    /// The release notes on openwrt.org.
    pub notes: String,
    /// The device's image builder profile in that release (such as `glinet_gl-mt6000`); `None`
    /// when the release has no image for the board.
    pub profile: Option<String>,
    /// The official sysupgrade image's size: an image with the node's packages is larger.
    pub image_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OfferKind {
    /// A newer release of the branch the node runs: fixes only.
    Update,
    /// The next branch's latest release: new features, and changes to read first.
    Branch,
}

/// What an upgrade image will hold, against what the node runs: worked out before anything is
/// built, so dropped packages are seen first (D-121).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpgradePlan {
    pub node: uniwrt_core::NodeId,
    pub source: ImageSource,
    /// The release it runs, and the one the image is for.
    pub from: String,
    pub release: String,
    pub target: String,
    pub profile: String,
    /// What ASU is asked to add to the profile's defaults, or (with a leading `-`) remove; with
    /// an official image, what is installed (or removed) after the upgrade. Empty for an upload.
    pub packages: Vec<String>,
    /// The node's own packages carried over (installed on purpose, beyond the defaults).
    pub kept: Vec<String>,
    /// Packages the new release names differently (ASU's published changes).
    pub renamed: Vec<PackageRename>,
    /// What uniwrt adds: the mesh essentials the node lacks.
    pub added: Vec<String>,
    /// The node's packages the new release doesn't have: left out of the image.
    pub dropped: Vec<String>,
    /// Defaults of the profile the node had removed: left out of the image too.
    pub removed: Vec<String>,
    /// When the node's packages were read; `None` for an upload, which reads none.
    pub packages_at: Option<uniwrt_core::Timestamp>,
    /// The official sysupgrade image's size, and the node's free RAM in `/tmp` where the
    /// image is staged: an image with more packages is larger.
    pub image_bytes: Option<u64>,
    pub tmp_free_bytes: Option<u64>,
    /// An official image's files on the download server (the sysupgrade one first; x86 has a
    /// BIOS and an EFI one per file system). Empty for the other sources.
    pub images: Vec<FirmwareImage>,
    /// The uploaded image, for [`ImageSource::Upload`].
    pub upload: Option<FirmwareUpload>,
}

/// Where an upgrade image comes from (D-121, D-122).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// Built by ASU with the node's packages baked in.
    Asu,
    /// OpenWrt's own image; the node's packages are installed after the upgrade.
    Official,
    /// An image an admin uploaded, flashed as it is.
    Upload,
}

/// An image an admin uploaded (D-122), described by its metadata: OpenWrt appends the boards
/// it fits and its release to every sysupgrade image but x86's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirmwareUpload {
    /// Its SHA-256 in lowercase hex: its identity.
    pub sha256: String,
    /// The file's name as uploaded.
    pub name: String,
    pub size_bytes: u64,
    pub uploaded_at: uniwrt_core::Timestamp,
    /// The target and subtarget, such as `mediatek/filogic` (an x86 image's from its name).
    pub target: String,
    /// The boards it fits (`glinet,gl-mt6000`); empty for an x86 image, which fits any.
    pub boards: Vec<String>,
    /// From the metadata, when the image has some.
    pub distribution: Option<String>,
    pub version: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PackageRename {
    pub from: String,
    pub to: String,
}

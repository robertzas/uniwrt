//! Upgrades (M7.1a1, D-121): the releases on offer for a node, what an upgrade image keeps of
//! its packages, and the build. A target's devices come from the download server's
//! `profiles.json`, the packages a release has from ASU's indexes; both are kept a day.

use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    facts::Release,
    firmware::{
        FirmwareImage, FirmwareOffer, ImageBuildStarted, ImageSource, PackageRename, UpgradePlan,
    },
    inventory::Package,
};

use super::{Asu, MAX_CATALOG};
use crate::{
    Error, build,
    catalog::{Branch, valid_target},
    device::{self, Target, valid_arch},
    keep, release,
};

/// How long a target's devices and a release's package indexes are kept: a release doesn't
/// change.
const TTL: Duration = Duration::from_hours(24);
/// The targets kept (a fleet has few), and the package indexes.
const MAX_TARGETS: usize = 8;
const MAX_INDEXES: usize = 4;

/// A node to upgrade: its firmware, its packages, and the release to upgrade to.
#[derive(Debug, Clone, Copy)]
pub struct Upgrade<'a> {
    pub node: NodeId,
    pub release: &'a Release,
    /// The board images target (`glinet,gl-mt6000`).
    pub board: &'a str,
    /// One of [`Asu::offers`]'s releases.
    pub to: &'a str,
    pub packages: &'a [Package],
    /// When the packages were read.
    pub packages_at: Timestamp,
    /// Free RAM in `/tmp`, where the image is staged.
    pub tmp_free_bytes: Option<u64>,
}

impl Asu {
    /// The releases on offer for a node running `release` on `board`, newest first, with the
    /// device's profile in each; and why nothing can be offered, when that is so.
    pub async fn offers(
        &self,
        release: &Release,
        board: &str,
    ) -> Result<(Vec<FirmwareOffer>, Option<String>), Error> {
        if release.distribution != "OpenWrt" {
            let note = format!(
                "{} isn't OpenWrt: ASU builds OpenWrt only",
                release.distribution
            );
            return Ok((Vec::new(), Some(note)));
        }
        if !valid_target(&release.target) {
            let note = format!("{} isn't a target OpenWrt builds", release.target);
            return Ok((Vec::new(), Some(note)));
        }
        let overview = self.overview().await?;
        let offered = match release::offers(&release.version, &overview.branches) {
            Ok(offered) => offered,
            Err(note) => return Ok((Vec::new(), Some(note))),
        };
        let mut offers = Vec::with_capacity(offered.len());
        for (to, kind) in offered {
            let device = match self.target(&to, &release.target).await {
                Ok(target) => target
                    .device(board)
                    .map(|device| (device.id.clone(), device.sysupgrade_bytes)),
                Err(Error::NotFound(_)) => None,
                Err(err) => return Err(err),
            };
            let (profile, image_bytes) = device.unzip();
            offers.push(FirmwareOffer {
                notes: release::notes(&to),
                release: to,
                kind,
                profile,
                image_bytes: image_bytes.flatten(),
            });
        }
        let note = (!offers.is_empty() && offers.iter().all(|offer| offer.profile.is_none()))
            .then(|| format!("OpenWrt's newer releases have no image for {board}"));
        Ok((offers, note))
    }

    /// What an image of `upgrade.to` would hold against the node's packages.
    pub async fn upgrade_plan(&self, upgrade: Upgrade<'_>) -> Result<UpgradePlan, Error> {
        let (offers, note) = self.offers(upgrade.release, upgrade.board).await?;
        let offer = offers
            .iter()
            .find(|offer| offer.release == upgrade.to)
            .ok_or_else(|| {
                Error::NotFound(note.clone().unwrap_or_else(|| {
                    format!(
                        "{} isn't an upgrade on offer for {}",
                        upgrade.to, upgrade.release.version
                    )
                }))
            })?;
        let no_image = || {
            Error::NotFound(format!(
                "OpenWrt {} has no image for {}",
                upgrade.to, upgrade.board
            ))
        };
        let profile = offer.profile.clone().ok_or_else(no_image)?;
        let target = upgrade.release.target.as_str();
        let new = self.target(upgrade.to, target).await?;
        let device = new.device(upgrade.board).ok_or_else(no_image)?;
        let new_defaults = new.packages(device);
        // An old release may be gone from the download server: then no default is removed.
        let old_defaults = match self.target(&upgrade.release.version, target).await {
            Ok(old) => old.device(upgrade.board).map(|device| old.packages(device)),
            Err(_) => None,
        };
        let available = self.index(upgrade.to, &new.arch, target).await?;
        let overview = self.overview().await?;
        let branch = |version: &str| -> Option<&Branch> {
            let name = branch_name(version);
            overview.branches.iter().find(|branch| branch.name == name)
        };
        let (from, to) = (branch(&upgrade.release.version), branch(upgrade.to));
        let mut unnamed = BTreeSet::new();
        let mut explicit = BTreeSet::new();
        for package in upgrade.packages.iter().filter(|package| package.explicit) {
            if valid_package(&package.name) {
                explicit.insert(package.name.clone());
            } else {
                unnamed.insert(package.name.clone());
            }
        }
        let kept = keep::plan(&keep::Inputs {
            explicit: &explicit,
            old_defaults: old_defaults.as_ref(),
            new_defaults: &new_defaults,
            available: &available,
            changes: to.map_or(&[], |branch| branch.changes.as_slice()),
            from_revision: from.and_then(|branch| branch.forked_at),
            to_revision: to.and_then(|branch| branch.forked_at),
        });
        let mut dropped = kept.dropped;
        dropped.extend(unnamed);
        dropped.sort();
        Ok(UpgradePlan {
            node: upgrade.node,
            source: ImageSource::Asu,
            from: upgrade.release.version.clone(),
            release: upgrade.to.to_owned(),
            target: target.to_owned(),
            profile,
            packages: kept.packages,
            kept: kept.kept,
            renamed: kept
                .renamed
                .into_iter()
                .map(|(from, to)| PackageRename { from, to })
                .collect(),
            added: kept.added,
            dropped,
            removed: kept.removed,
            packages_at: Some(upgrade.packages_at),
            image_bytes: offer.image_bytes,
            tmp_free_bytes: upgrade.tmp_free_bytes,
            images: Vec::new(),
            upload: None,
        })
    }

    /// The plan of an upgrade with OpenWrt's own image (D-122): the same packages, installed
    /// after the upgrade rather than baked in, and the image's files on the download server.
    pub async fn official_plan(&self, upgrade: Upgrade<'_>) -> Result<UpgradePlan, Error> {
        let mut plan = self.upgrade_plan(upgrade).await?;
        let target = self.target(&plan.release, &plan.target).await?;
        let device = target.device(upgrade.board).ok_or_else(|| {
            Error::NotFound(format!(
                "OpenWrt {} has no image for {}",
                plan.release, upgrade.board
            ))
        })?;
        if device.images.is_empty() {
            return Err(Error::NotFound(format!(
                "OpenWrt {} lists no sysupgrade image for {}",
                plan.release, upgrade.board
            )));
        }
        let upstream = self.overview().await?.upstream;
        plan.source = ImageSource::Official;
        plan.images = device
            .images
            .iter()
            .map(|image| FirmwareImage {
                url: format!(
                    "{upstream}/releases/{}/targets/{}/{}",
                    plan.release, plan.target, image.name
                ),
                name: image.name.clone(),
                kind: image.kind.clone(),
                filesystem: image.filesystem.clone(),
                size_bytes: image.size,
                sha256: image.sha256.clone(),
            })
            .collect();
        if let Some(size) = plan.images[0].size_bytes {
            plan.image_bytes = Some(size);
        }
        Ok(plan)
    }

    /// Asks ASU for the upgrade image [`Self::upgrade_plan`] describes; the node's
    /// configuration is kept by sysupgrade, so no first-boot script goes with it.
    pub async fn build_upgrade(&self, upgrade: Upgrade<'_>) -> Result<ImageBuildStarted, Error> {
        let plan = self.upgrade_plan(upgrade).await?;
        let request = build::Request {
            version: &plan.release,
            target: &plan.target,
            profile: &plan.profile,
            packages: &plan.packages,
            defaults: None,
            client: &self.inner.client,
        };
        let build = self.post_build(&request).await?;
        Ok(ImageBuildStarted {
            packages: plan.packages,
            build,
        })
    }

    /// `target`'s devices in `release`, from the download server.
    async fn target(&self, release: &str, target: &str) -> Result<Arc<Target>, Error> {
        if release::Version::parse(release).is_none() || !valid_target(target) {
            return Err(Error::Invalid(format!(
                "{release} {target} isn't a release's target"
            )));
        }
        let key = format!("{release} {target}");
        if let Some(cached) = fresh(&self.cache().targets, &key) {
            return Ok(cached);
        }
        let overview = self.overview().await?;
        let url = format!(
            "{}/releases/{release}/targets/{target}/profiles.json",
            overview.upstream
        );
        let parsed = Arc::new(device::target(&self.get(&url, MAX_CATALOG).await?)?);
        remember(
            &mut self.cache().targets,
            key,
            Arc::clone(&parsed),
            MAX_TARGETS,
        );
        Ok(parsed)
    }

    /// The packages `release` has for `arch` (its feeds') and `target` (its kernel modules).
    async fn index(
        &self,
        release: &str,
        arch: &str,
        target: &str,
    ) -> Result<Arc<BTreeSet<String>>, Error> {
        if !valid_arch(arch) {
            return Err(Error::Invalid(format!(
                "{arch} isn't a package architecture"
            )));
        }
        let key = format!("{release} {arch} {target}");
        if let Some(cached) = fresh(&self.cache().indexes, &key) {
            return Ok(cached);
        }
        let base = format!("{}/json/v1/releases/{release}", self.inner.base);
        let mut names = device::index(
            &self
                .get(&format!("{base}/packages/{arch}-index.json"), MAX_CATALOG)
                .await?,
        )?;
        names.extend(device::index(
            &self
                .get(&format!("{base}/targets/{target}/index.json"), MAX_CATALOG)
                .await?,
        )?);
        let names = Arc::new(names);
        remember(
            &mut self.cache().indexes,
            key,
            Arc::clone(&names),
            MAX_INDEXES,
        );
        Ok(names)
    }
}

/// The branch a release belongs to: `25.12` for `25.12.6`.
fn branch_name(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// A package's name as ASU takes it: what a node reports is checked before it is sent.
fn valid_package(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'))
}

type Entries<T> = Vec<(String, Instant, Arc<T>)>;

fn fresh<T>(entries: &Entries<T>, key: &str) -> Option<Arc<T>> {
    let now = Instant::now();
    entries
        .iter()
        .find(|(known, at, _)| known == key && now.duration_since(*at) < TTL)
        .map(|(_, _, value)| Arc::clone(value))
}

fn remember<T>(entries: &mut Entries<T>, key: String, value: Arc<T>, max: usize) {
    entries.retain(|(known, _, _)| *known != key);
    entries.push((key, Instant::now(), value));
    if entries.len() > max {
        entries.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_from_a_node_are_checked() {
        assert!(valid_package("luci-app-sqm"));
        assert!(valid_package("libstdcpp6"));
        assert!(valid_package("kmod-nf-conntrack6"));
        assert!(valid_package("g++"));
        assert!(!valid_package("-wpad"));
        assert!(!valid_package("a b"));
        assert!(!valid_package(""));
        assert_eq!(branch_name("25.12.0-rc2"), "25.12");
    }
}

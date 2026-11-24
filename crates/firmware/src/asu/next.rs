//! What a node would need on the releases it is offered (D-147, M9.0c1b2), so the feed mirror
//! can keep it ahead: per offer with an image for the board, the plan of an upgrade with
//! OpenWrt's own image (the packages its reinstall asks for, the image's files) and what the
//! target's `profiles.json` tells of the release's feeds (the package architecture, the kernel
//! modules' directory, the package manager).

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{facts::Release, feeds::NextRelease, inventory::Package};

use super::{Asu, Upgrade};
use crate::Error;

impl Asu {
    /// The releases on offer for a node running `release` on `board` with `packages` (read at
    /// `packages_at`), each with what it would need there; an offer without an official image
    /// for the board is left out.
    pub async fn next_releases(
        &self,
        node: NodeId,
        release: &Release,
        board: &str,
        packages: &[Package],
        packages_at: Timestamp,
    ) -> Result<Vec<NextRelease>, Error> {
        let (offers, _) = self.offers(release, board).await?;
        let mut next = Vec::with_capacity(offers.len());
        for offer in offers.iter().filter(|offer| offer.profile.is_some()) {
            let upgrade = Upgrade {
                node,
                release,
                board,
                to: &offer.release,
                packages,
                packages_at,
                tmp_free_bytes: None,
            };
            let plan = match self.official_plan(upgrade).await {
                Ok(plan) => plan,
                Err(Error::NotFound(_)) => continue,
                Err(err) => return Err(err),
            };
            let target = self.target(&plan.release, &plan.target).await?;
            next.push(NextRelease {
                release: plan.release,
                target: plan.target,
                arch: target.arch.clone(),
                manager: target.manager,
                kmods: target.kmods.clone(),
                packages: plan.packages,
                images: plan.images,
            });
        }
        Ok(next)
    }
}

#[cfg(test)]
#[path = "next_tests.rs"]
mod tests;

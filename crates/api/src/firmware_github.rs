//! GitHub releases in the firmware catalog and plans (M7.1a2b, D-122): a node whose source is
//! a GitHub repo (its own, or the site's) is offered the repo's newest releases with an image
//! for it instead of OpenWrt's; its plan names the release's images, flashed as they are.

use std::{collections::BTreeMap, sync::Arc};

use uniwrt_firmware::{Device, Error, Releases};
use uniwrt_model::{
    facts::NodeFacts,
    firmware::{FirmwareSources, ImageSource, NodeFirmware, UpgradePlan},
    node::Node,
};
use zeroize::Zeroizing;

use crate::{Api, ApiError, ApiResult, firmware::firmware_error, firmware_sources};

/// What one catalog read knows of GitHub: the sources, the token, and each repo read once.
pub(crate) struct Sources {
    pub(crate) kept: FirmwareSources,
    token: Option<Zeroizing<String>>,
    read: BTreeMap<String, Result<Arc<Releases>, String>>,
}

impl Sources {
    pub(crate) async fn load(api: &Api) -> ApiResult<Self> {
        Ok(Self {
            kept: firmware_sources::load(api).await?,
            token: firmware_sources::token(api).await?,
            read: BTreeMap::new(),
        })
    }

    pub(crate) fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Lists the releases of the node's repo with an image for it in `entry` (or why none);
    /// `profile` is its image builder profile, when known.
    pub(crate) async fn fill(
        &mut self,
        api: &Api,
        entry: &mut NodeFirmware,
        facts: &NodeFacts,
        profile: Option<&str>,
    ) {
        let Some(repo) = self.kept.repo_of(entry.node).map(str::to_owned) else {
            return;
        };
        entry.repo = Some(repo.clone());
        let releases = if let Some(read) = self.read.get(&repo) {
            read.clone()
        } else {
            let read = match api.github() {
                Ok(github) => github
                    .releases(&repo, self.token.as_deref().map(String::as_str))
                    .await
                    .map_err(|err| note(&repo, err)),
                Err(err) => Err(err.to_string()),
            };
            self.read.insert(repo, read.clone());
            read
        };
        match releases {
            Ok(releases) => {
                let (offers, note) = releases.offers(device(facts, profile), self.kept.prereleases);
                entry.releases = offers;
                entry.note = note;
            }
            Err(why) => entry.note = Some(why),
        }
    }
}

/// Why a repo's releases can't be listed, short enough for every row: a transport failure is
/// logged whole once, the rest (no such repo, the token, the rate limit) say it in a sentence.
fn note(repo: &str, err: Error) -> String {
    match err {
        Error::Unreachable { .. } | Error::Status { .. } | Error::Invalid(_) => {
            tracing::warn!(%repo, %err, "cannot read the GitHub repo's releases");
            format!("Cannot read the releases of {repo} from GitHub now: try again later")
        }
        other => other.to_string(),
    }
}

fn device<'a>(facts: &'a NodeFacts, profile: Option<&'a str>) -> Device<'a> {
    Device {
        target: &facts.board.release.target,
        board: &facts.board.board_name,
        profile,
    }
}

/// The plan of upgrading `node` with its repo's release `tag`: one of those on offer for it.
pub(crate) async fn plan(
    api: &Api,
    node: &Node,
    facts: &NodeFacts,
    tag: &str,
) -> ApiResult<UpgradePlan> {
    let sources = firmware_sources::load(api).await?;
    let repo = sources.repo_of(node.id).ok_or_else(|| {
        ApiError::Conflict(format!(
            "{} takes its images from OpenWrt, not from GitHub",
            node.name
        ))
    })?;
    let token = firmware_sources::token(api).await?;
    let listed = api
        .github()?
        .releases(repo, token.as_deref().map(String::as_str))
        .await
        .map_err(firmware_error)?;
    let profile = match api.asu() {
        Ok(asu) => asu
            .profile_of(&facts.board.release, &facts.board.board_name)
            .await
            .ok()
            .flatten(),
        Err(_) => None,
    };
    let (offers, why) = listed.offers(device(facts, profile.as_deref()), sources.prereleases);
    let release = offers
        .into_iter()
        .find(|release| release.tag == tag)
        .ok_or_else(|| {
            ApiError::NotFound(why.unwrap_or_else(|| {
                format!("{tag} isn't a release of {repo} on offer for {}", node.name)
            }))
        })?;
    Ok(UpgradePlan {
        node: node.id,
        source: ImageSource::Github,
        from: facts.board.release.version.clone(),
        release: release.tag,
        target: facts.board.release.target.clone(),
        profile: profile.unwrap_or_default(),
        packages: Vec::new(),
        kept: Vec::new(),
        renamed: Vec::new(),
        added: Vec::new(),
        dropped: Vec::new(),
        removed: Vec::new(),
        packages_at: None,
        image_bytes: release.images.first().and_then(|image| image.size_bytes),
        tmp_free_bytes: facts.system.tmp.map(|tmp| tmp.available_bytes),
        images: release.images,
        upload: None,
        repo: Some(repo.to_owned()),
    })
}

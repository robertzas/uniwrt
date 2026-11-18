//! A rollout's images (D-125): [`RolloutImages`] over the install's sources. At a node's turn
//! its upgrade is planned again from the node (`firmware_install::upgrade_request`, as an
//! install resolves it); for ASU, the build is asked for again (ASU answers the same request
//! with the same, cached build) and waited for, up to [`BUILD_WAIT`]. Ahead of the turns, the
//! rollout's start asks ASU for each build, so most are ready when their node's turn comes.

use std::time::Duration;

use axum::extract::Path;
use uniwrt_core::NodeId;
use uniwrt_jobs::{BoxFuture, RolloutImages, UpgradeRequest};
use uniwrt_model::{firmware::ImageBuildState, rollout::UpgradeChoice};

use crate::{
    ApiResult,
    firmware::{firmware_error, node_facts, read, upgrade},
    firmware_install::{UpgradeInstallBody, upgrade_request},
    state::WeakApi,
};

/// How long a node's turn waits for ASU's build.
const BUILD_WAIT: Duration = Duration::from_mins(45);
/// How often it asks ASU how the build stands.
const BUILD_POLL: Duration = Duration::from_secs(10);

/// The API's image sources, for the rollout driver (which the API holds: hence the weak link).
pub(crate) struct ApiImages(pub(crate) WeakApi);

impl ApiImages {
    fn api(&self) -> Result<crate::Api, String> {
        self.0
            .upgrade()
            .ok_or_else(|| "The controller is stopping".to_owned())
    }
}

impl RolloutImages for ApiImages {
    fn ahead<'a>(
        &'a self,
        node: NodeId,
        choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let UpgradeChoice::Asu { release } = choice else {
                return Ok(());
            };
            build(&self.api()?, node, release)
                .await
                .map(drop)
                .map_err(|err| err.to_string())
        })
    }

    fn request<'a>(
        &'a self,
        node: NodeId,
        choice: &'a UpgradeChoice,
    ) -> BoxFuture<'a, Result<UpgradeRequest, String>> {
        Box::pin(async move {
            let api = self.api()?;
            let body = match choice.clone() {
                UpgradeChoice::Asu { release } => {
                    let build = built(&api, node, &release).await?;
                    UpgradeInstallBody::Asu { release, build }
                }
                UpgradeChoice::Official { release } => UpgradeInstallBody::Official { release },
                UpgradeChoice::Upload { sha256 } => UpgradeInstallBody::Upload { sha256 },
                UpgradeChoice::Github { release } => UpgradeInstallBody::Github { release },
            };
            let (found, facts) = node_facts(&api, Ok(Path(node.get())))
                .await
                .map_err(|err| err.to_string())?;
            upgrade_request(&api, &found, &facts, body)
                .await
                .map(|(request, _)| request)
                .map_err(|err| err.to_string())
        })
    }
}

/// Asks ASU to build the node's upgrade to `release` with its packages read now; the build's
/// hash.
async fn build(api: &crate::Api, node: NodeId, release: &str) -> ApiResult<String> {
    let (node, facts, inventory) = read(api, Ok(Path(node.get())), release).await?;
    let started = api
        .asu()?
        .build_upgrade(upgrade(node.id, &facts, &inventory, release))
        .await
        .map_err(firmware_error)?;
    Ok(started.build.hash)
}

/// [`build`], then waits for it to finish.
async fn built(api: &crate::Api, node: NodeId, release: &str) -> Result<String, String> {
    let hash = build(api, node, release)
        .await
        .map_err(|err| err.to_string())?;
    let asu = api.asu().map_err(|err| err.to_string())?;
    let waited = tokio::time::timeout(BUILD_WAIT, async {
        loop {
            let build = asu
                .status(&hash)
                .await
                .map_err(|err| firmware_error(err).to_string())?;
            match build.state {
                ImageBuildState::Done => return Ok(()),
                ImageBuildState::Failed => {
                    return Err(format!(
                        "ASU couldn't build its image: {}",
                        build.error.as_deref().unwrap_or("no reason given")
                    ));
                }
                ImageBuildState::Queued | ImageBuildState::Building => {
                    tokio::time::sleep(BUILD_POLL).await;
                }
            }
        }
    })
    .await;
    match waited {
        Ok(Ok(())) => Ok(hash),
        Ok(Err(why)) => Err(why),
        Err(_) => Err(format!(
            "ASU didn't finish its image within {} minutes",
            BUILD_WAIT.as_secs() / 60
        )),
    }
}

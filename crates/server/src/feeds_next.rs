//! The upgrades the feed mirror keeps ahead (D-147, M9.0c1b2): OpenWrt's releases on offer for
//! each node whose images come from OpenWrt (not from a GitHub repo), read from ASU's catalog
//! (kept on disk, so offline lookups answer with what was read last).

use uniwrt_core::NodeId;
use uniwrt_engine::NextReleases;
use uniwrt_feeds::BoxFuture;
use uniwrt_firmware::Asu;
use uniwrt_model::{
    facts::NodeFacts, feeds::NextRelease, firmware::FirmwareSources, inventory::Inventory,
};
use uniwrt_store::{SettingsRepo as _, Store};

pub(crate) struct OfficialNext {
    pub(crate) asu: Asu,
    pub(crate) store: Store,
}

impl NextReleases for OfficialNext {
    fn next<'a>(
        &'a self,
        node: NodeId,
        facts: &'a NodeFacts,
        inventory: &'a Inventory,
    ) -> BoxFuture<'a, Result<Vec<NextRelease>, String>> {
        Box::pin(async move {
            let sources: FirmwareSources = match self.store.setting(FirmwareSources::KEY).await {
                Ok(Some(value)) => serde_json::from_value(value).unwrap_or_default(),
                Ok(None) => FirmwareSources::default(),
                Err(err) => return Err(err.to_string()),
            };
            if sources.repo_of(node).is_some() {
                return Ok(Vec::new());
            }
            self.asu
                .next_releases(
                    node,
                    &facts.board.release,
                    &facts.board.board_name,
                    &inventory.packages,
                    inventory.taken_at,
                )
                .await
                .map_err(|err| err.to_string())
        })
    }
}

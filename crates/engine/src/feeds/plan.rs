//! What the mirror keeps (D-147): the nodes' feed sets, their feeds and the names wanted.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_feeds::{Feed, Plan, SetPlan};
use uniwrt_model::{
    diag::tools::OptionalTool, facts::NodeFacts, feeds::FeedSet, inventory::Inventory,
};
use uniwrt_node::wpad;

/// The mesh essentials (D-104): what a node needs to join the mesh.
const MESH: [&str; 2] = ["kmod-batman-adv", "batctl-default"];

/// The names wanted of every feed set: what uniwrt can install.
fn installable() -> BTreeSet<String> {
    uniwrt_render::every_package()
        .into_iter()
        .chain(OptionalTool::ALL.map(OptionalTool::package))
        .chain(MESH)
        .map(str::to_owned)
        .collect()
}

/// The plan for `nodes` (each one's facts and inventory), `whole` keeping every package.
pub fn plan<'a>(
    nodes: impl IntoIterator<Item = (&'a NodeFacts, &'a Inventory)>,
    whole: bool,
) -> Plan {
    let mut sets: BTreeMap<FeedSet, SetPlan> = BTreeMap::new();
    for (facts, inventory) in nodes {
        let Some(manager) = facts.capabilities.package_manager else {
            continue;
        };
        let release = &facts.board.release;
        if [&release.version, &release.target, &release.arch]
            .iter()
            .any(|part| part.is_empty())
        {
            continue;
        }
        let feeds: Vec<Feed> = inventory
            .feeds
            .iter()
            .filter_map(|feed| Feed::parse(&feed.url).ok())
            .collect();
        if feeds.is_empty() {
            continue;
        }
        let set = FeedSet {
            release: release.version.clone(),
            target: release.target.clone(),
            arch: release.arch.clone(),
            manager,
        };
        let entry = sets.entry(set.clone()).or_insert_with(|| SetPlan {
            set,
            feeds: Vec::new(),
            wanted: installable(),
        });
        for feed in feeds {
            if !entry.feeds.contains(&feed) {
                entry.feeds.push(feed);
            }
        }
        let packages = &inventory.packages;
        entry.wanted.extend(
            packages
                .iter()
                .filter(|package| package.explicit)
                .map(|package| package.name.clone()),
        );
        let installed: BTreeSet<String> = packages.iter().map(|p| p.name.clone()).collect();
        if let Ok(Some(swap)) = wpad::choose(manager, &installed) {
            entry.wanted.insert(swap.to);
        }
    }
    Plan {
        sets: sets.into_values().collect(),
        whole,
    }
}

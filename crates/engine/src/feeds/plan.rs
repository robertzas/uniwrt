//! What the mirror keeps (D-147): the nodes' feed sets, their feeds and the names wanted, and
//! those of the releases they are offered (M9.0c1b2) with their official images.

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_feeds::{Feed, Plan, SetPlan};
use uniwrt_model::{
    diag::tools::OptionalTool,
    facts::NodeFacts,
    feeds::{FeedSet, NextRelease},
    firmware::FirmwareImage,
    inventory::Inventory,
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

/// The plan for `nodes` (each one's facts, inventory and the releases it is offered), `whole`
/// keeping every package.
pub fn plan<'a>(
    nodes: impl IntoIterator<Item = (&'a NodeFacts, &'a Inventory, &'a [NextRelease])>,
    whole: bool,
) -> Plan {
    let mut sets: BTreeMap<FeedSet, SetPlan> = BTreeMap::new();
    let mut images: Vec<FirmwareImage> = Vec::new();
    for (facts, inventory, offered) in nodes {
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
        for next in offered {
            plan_next(&mut sets, &mut images, &feeds, facts, manager, next);
        }
        let entry = sets.entry(set.clone()).or_insert_with(|| SetPlan {
            set,
            feeds: Vec::new(),
            wanted: installable(),
            offered: false,
        });
        entry.offered = false;
        add_feeds(entry, feeds);
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
        images,
    }
}

fn add_feeds(entry: &mut SetPlan, feeds: impl IntoIterator<Item = Feed>) {
    for feed in feeds {
        if !entry.feeds.contains(&feed) {
            entry.feeds.push(feed);
        }
    }
}

/// The feed set of `next`, offered to a node with `feeds` and `facts`: its feeds derived from
/// the node's, wanting what the reinstall after OpenWrt's image asks for; and that image.
fn plan_next(
    sets: &mut BTreeMap<FeedSet, SetPlan>,
    images: &mut Vec<FirmwareImage>,
    feeds: &[Feed],
    facts: &NodeFacts,
    manager: uniwrt_model::facts::PackageManager,
    next: &NextRelease,
) {
    let release = &facts.board.release;
    let derived = uniwrt_feeds::next::derive(feeds, &release.version, &release.arch, next);
    if derived.is_empty() {
        return;
    }
    let set = FeedSet {
        release: next.release.clone(),
        target: next.target.clone(),
        arch: next.arch.clone(),
        manager: next.manager.unwrap_or(manager),
    };
    let entry = sets.entry(set.clone()).or_insert_with(|| SetPlan {
        set,
        feeds: Vec::new(),
        wanted: installable(),
        offered: true,
    });
    add_feeds(entry, derived);
    entry.wanted.extend(
        next.packages
            .iter()
            .filter(|name| !name.starts_with('-'))
            .cloned(),
    );
    for image in &next.images {
        if !images.iter().any(|kept| kept.url == image.url) {
            images.push(image.clone());
        }
    }
}

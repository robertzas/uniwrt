//! The releases nodes are offered (D-147, M9.0c1b2), so the mirror keeps what an upgrade to
//! one would need: asked of a [`NextReleases`] (the server's asks OpenWrt's catalog) per node
//! at most once per [`FeedMirrorConfig::next_every`](super::FeedMirrorConfig::next_every),
//! and again as soon as the node runs another release. A failed lookup keeps the last answer
//! for the release the node runs. Until every node running a known release has been looked
//! up once, the plan in force stays (none at start), so a start never sweeps a next release's
//! copies for want of an answer.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tracing::warn;
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_feeds::BoxFuture;
use uniwrt_model::{facts::NodeFacts, feeds::NextRelease, inventory::Inventory};

/// Where the releases on offer come from.
pub trait NextReleases: Send + Sync + 'static {
    /// The releases `node` is offered, each with what it would need there; none when its
    /// images don't come from OpenWrt.
    fn next<'a>(
        &'a self,
        node: NodeId,
        facts: &'a NodeFacts,
        inventory: &'a Inventory,
    ) -> BoxFuture<'a, Result<Vec<NextRelease>, String>>;
}

/// A [`NextReleases`] in the mirror's configuration.
#[derive(Clone)]
pub struct NextSource(pub Arc<dyn NextReleases>);

impl std::fmt::Debug for NextSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NextSource")
    }
}

/// The release a node runs, when it can be offered another.
pub(super) fn running(facts: &NodeFacts) -> Option<&str> {
    let version = facts.board.release.version.as_str();
    (!version.is_empty() && !facts.board.board_name.is_empty()).then_some(version)
}

/// What each node is offered, as last looked up.
#[derive(Default)]
pub(super) struct Lookups {
    /// The last answer per node, with the release it ran then.
    answers: BTreeMap<NodeId, (String, Vec<NextRelease>)>,
    /// When each node was last asked about, running which release.
    asked: BTreeMap<NodeId, (String, Timestamp)>,
    running: BTreeSet<NodeId>,
}

impl Lookups {
    /// Whether `node`, running `version`, is due a lookup.
    pub(super) fn due(&self, node: NodeId, version: &str, now: Timestamp, every: Duration) -> bool {
        !self.running.contains(&node)
            && self.asked.get(&node).is_none_or(|(asked, at)| {
                asked != version || now.saturating_duration_since(*at) >= every
            })
    }

    pub(super) fn start(&mut self, node: NodeId, version: String, now: Timestamp) {
        self.asked.insert(node, (version, now));
        self.running.insert(node);
    }

    pub(super) fn finish(
        &mut self,
        node: NodeId,
        version: String,
        answer: Result<Vec<NextRelease>, String>,
    ) {
        self.running.remove(&node);
        match answer {
            Ok(next) => {
                self.answers.insert(node, (version, next));
            }
            Err(why) => warn!(%node, %why, "feed mirror: cannot read the node's upgrades on offer"),
        }
    }

    /// Whether `node`, running `version`, was looked up (answered or not).
    pub(super) fn settled(&self, node: NodeId, version: &str) -> bool {
        !self.running.contains(&node)
            && self
                .asked
                .get(&node)
                .is_some_and(|(asked, _)| asked == version)
    }

    /// No lookup runs.
    pub(super) fn idle(&self) -> bool {
        self.running.is_empty()
    }

    /// What `node`, running `version`, is offered, as last answered.
    pub(super) fn of(&self, node: NodeId, version: &str) -> &[NextRelease] {
        self.answers
            .get(&node)
            .filter(|(answered, _)| answered == version)
            .map_or(&[], |(_, next)| next.as_slice())
    }

    /// Forgets the nodes `keep` doesn't name.
    pub(super) fn retain(&mut self, keep: impl Fn(NodeId) -> bool) {
        self.answers.retain(|node, _| keep(*node));
        self.asked.retain(|node, _| keep(*node));
    }
}

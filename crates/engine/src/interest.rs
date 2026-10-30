//! Interest in a node: while someone holds an [`Interest`], its telemetry is fast
//! ([`Engine::focus`](crate::Engine::focus)) or its log is followed
//! ([`Engine::follow_logs`](crate::Engine::follow_logs)). The node's actor watches the counts.

use std::sync::Arc;

use tokio::sync::watch;

/// How many hold each kind of interest in one node ([`Engine::interests`](crate::Engine::interests)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Interests {
    /// Views of the node open in the UI.
    pub focus: usize,
    /// Followers of its log.
    pub logs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Focus,
    Logs,
}

/// Holds interest in a node until dropped.
#[derive(Debug)]
#[must_use = "the interest ends when this is dropped"]
pub struct Interest {
    counts: Arc<watch::Sender<Interests>>,
    kind: Kind,
}

impl Interest {
    pub(crate) fn hold(counts: &Arc<watch::Sender<Interests>>, kind: Kind) -> Self {
        counts.send_modify(|counts| *count(counts, kind) += 1);
        Self {
            counts: Arc::clone(counts),
            kind,
        }
    }
}

impl Drop for Interest {
    fn drop(&mut self) {
        self.counts
            .send_modify(|counts| *count(counts, self.kind) -= 1);
    }
}

fn count(counts: &mut Interests, kind: Kind) -> &mut usize {
    match kind {
        Kind::Focus => &mut counts.focus,
        Kind::Logs => &mut counts.logs,
    }
}

//! What the topology's history keeps of a build, for the replay (features/observability.md,
//! Topology): its shape, states and link quality, without what changes at every build.

use crate::{TopoEdge, TopoNode, Topology};

/// Signals are kept to this many dB, so that noise does not count as a change.
const SIGNAL_STEP_DBM: i16 = 2;
/// Quality is kept to this many points of 100.
const QUALITY_STEP: u8 = 5;

impl Topology {
    /// The build without traffic and load, its signals rounded to 2 dB and its quality to 5
    /// points: two builds of a network that looks the same are then equal.
    #[must_use]
    pub fn for_history(&self) -> Self {
        Self {
            at: self.at,
            nodes: self.nodes.iter().map(node_for_history).collect(),
            edges: self.edges.iter().map(edge_for_history).collect(),
        }
    }
}

fn node_for_history(node: &TopoNode) -> TopoNode {
    TopoNode {
        cpu_percent: None,
        memory_percent: None,
        ..node.clone()
    }
}

fn edge_for_history(edge: &TopoEdge) -> TopoEdge {
    TopoEdge {
        signal_dbm: edge.signal_dbm.map(|dbm| round_to(dbm, SIGNAL_STEP_DBM)),
        quality: edge
            .quality
            .map(|quality| round_to(i16::from(quality), i16::from(QUALITY_STEP)))
            .and_then(|quality| u8::try_from(quality).ok()),
        traffic: None,
        ..edge.clone()
    }
}

/// `value` to the nearest multiple of `step`, halves away from zero.
fn round_to(value: i16, step: i16) -> i16 {
    let half = if value < 0 { -step / 2 } else { step / 2 };
    value.saturating_add(half) / step * step
}

#[cfg(test)]
mod tests {
    use super::round_to;

    #[test]
    fn signals_and_quality_round_to_their_steps() {
        assert_eq!(round_to(-61, 2), -62);
        assert_eq!(round_to(-60, 2), -60);
        assert_eq!(round_to(-59, 2), -60);
        assert_eq!(round_to(62, 5), 60);
        assert_eq!(round_to(63, 5), 65);
        assert_eq!(round_to(100, 5), 100);
    }
}

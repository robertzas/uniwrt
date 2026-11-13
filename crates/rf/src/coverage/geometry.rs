//! Where placed nodes are in meters: each floor's plan scaled by its own scale, the floors stacked
//! by level with their heights, every plan's top-left corner over the same point (features/wifi.md,
//! Coverage & floor plans). The browser places points the same way (`features/floorplans/coverage`).

use uniwrt_core::NodeId;
use uniwrt_model::floor::{DEFAULT_HEIGHT_M, Floor};

/// A node placed on a scaled floor, in meters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Located {
    pub node: NodeId,
    pub level: i16,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Located {
    /// The straight-line distance, in meters.
    pub(super) fn distance(&self, other: &Self) -> f64 {
        (self.x - other.x)
            .hypot(self.y - other.y)
            .hypot(self.z - other.z)
    }

    /// The floors a signal between the two crosses.
    pub(super) fn floors(&self, other: &Self) -> u16 {
        self.level.abs_diff(other.level)
    }
}

/// The height of level `level`'s floor above the ground floor's, in meters: the heights of the
/// floors between, a missing floor counting [`DEFAULT_HEIGHT_M`].
pub(super) fn elevation(floors: &[Floor], level: i16) -> f64 {
    let height = |l: i16| {
        floors
            .iter()
            .find(|f| f.level == l)
            .map_or(DEFAULT_HEIGHT_M, |f| f.height_m)
    };
    if level >= 0 {
        (0..level).map(height).sum()
    } else {
        -(level..0).map(height).sum::<f64>()
    }
}

/// Every node placed on a floor that has a scale; nodes on floors without one have no meters.
pub(super) fn locate(floors: &[Floor]) -> Vec<Located> {
    floors
        .iter()
        .filter_map(|floor| floor.scale.map(|scale| (floor, scale.meters_per_px())))
        .filter(|(_, per_px)| per_px.is_finite() && *per_px > 0.0)
        .flat_map(|(floor, per_px)| {
            let z = elevation(floors, floor.level);
            floor.nodes.iter().map(move |p| Located {
                node: p.node,
                level: floor.level,
                x: p.at.x * per_px,
                y: p.at.y * per_px,
                z,
            })
        })
        .collect()
}

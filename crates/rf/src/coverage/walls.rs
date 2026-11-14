//! What the walls a signal crosses take from it (M6.2c). A path within a floor crosses that
//! floor's walls; a path between floors crosses each storey's walls on the part of it within the
//! storey's height, both ends taken [`ANTENNA_M`] above their floors. The browser counts them
//! the same way (`features/floorplans/coverage/walls.ts`).

use uniwrt_model::{
    floor::{Floor, Material},
    wifi::Band,
};

use super::geometry::{Located, elevation};

/// How high above its floor a path ends (a radio's antenna, a client in someone's hand), in
/// meters: a path to the floor above crosses that floor's walls over its last stretch.
pub(super) const ANTENNA_M: f64 = 1.0;

/// A wall in meters.
#[derive(Debug, Clone, Copy)]
struct Segment {
    a: (f64, f64),
    b: (f64, f64),
    material: Material,
}

/// A scaled floor's walls between its floor and the next one's.
#[derive(Debug, Clone)]
pub(super) struct Storey {
    level: i16,
    bottom: f64,
    top: f64,
    walls: Vec<Segment>,
}

/// Every scaled floor with walls.
pub(super) fn storeys(floors: &[Floor]) -> Vec<Storey> {
    floors
        .iter()
        .filter(|floor| !floor.walls.is_empty())
        .filter_map(|floor| {
            let per_px = floor.scale?.meters_per_px();
            (per_px.is_finite() && per_px > 0.0).then(|| {
                let bottom = elevation(floors, floor.level);
                Storey {
                    level: floor.level,
                    bottom,
                    top: bottom + floor.height_m,
                    walls: floor
                        .walls
                        .iter()
                        .map(|wall| Segment {
                            a: (wall.from.x * per_px, wall.from.y * per_px),
                            b: (wall.to.x * per_px, wall.to.y * per_px),
                            material: wall.material,
                        })
                        .collect(),
                }
            })
        })
        .collect()
}

fn cross(u: (f64, f64), v: (f64, f64)) -> f64 {
    u.0 * v.1 - u.1 * v.0
}

/// Whether the segments `p`–`q` and `a`–`b` meet, ends included (parallel ones never do).
pub(super) fn crosses(p: (f64, f64), q: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    let path = (q.0 - p.0, q.1 - p.1);
    let wall = (b.0 - a.0, b.1 - a.1);
    let det = cross(path, wall);
    if det.abs() < 1e-12 {
        return false;
    }
    let gap = (a.0 - p.0, a.1 - p.1);
    let along_path = cross(gap, wall) / det;
    let along_wall = cross(gap, path) / det;
    (0.0..=1.0).contains(&along_path) && (0.0..=1.0).contains(&along_wall)
}

/// The loss on `band` from the walls between `from` and `to`, in dB.
pub(super) fn wall_loss(storeys: &[Storey], from: &Located, to: &Located, band: Band) -> f64 {
    let (low, high) = (from.level.min(to.level), from.level.max(to.level));
    let dz = to.z - from.z;
    let at = |t: f64| (from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
    storeys
        .iter()
        .filter(|storey| (low..=high).contains(&storey.level))
        .map(|storey| {
            let (t0, t1) = if from.level == to.level || dz.abs() < 1e-9 {
                (0.0, 1.0)
            } else {
                let start = from.z + ANTENNA_M;
                let (a, b) = ((storey.bottom - start) / dz, (storey.top - start) / dz);
                (a.min(b).max(0.0), a.max(b).min(1.0))
            };
            if t1 < t0 {
                return 0.0;
            }
            let (p, q) = (at(t0), at(t1));
            storey
                .walls
                .iter()
                .filter(|wall| crosses(p, q, wall.a, wall.b))
                .map(|wall| wall.material.loss_db(band))
                .sum::<f64>()
        })
        .sum()
}

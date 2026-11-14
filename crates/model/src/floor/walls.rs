//! A floor's walls (features/wifi.md, Coverage & floor plans; M6.2c): segments drawn on the plan,
//! each of a material that takes a fixed loss per band from every signal crossing it. The
//! coverage model subtracts them before it fits the path loss and when it predicts.

use serde::{Deserialize, Serialize};

use super::{FloorImage, Point};
use crate::wifi::Band;

/// The most walls a floor keeps.
pub const MAX_WALLS: usize = 1000;
/// The shortest wall, in the plan's pixels.
pub const MIN_WALL_PX: f64 = 1.0;

/// A wall on a floor's plan, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct Wall {
    pub from: Point,
    pub to: Point,
    pub material: Material,
}

/// What a wall is made of, lightest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Glass,
    Drywall,
    Wood,
    Brick,
    Concrete,
    Metal,
}

impl Material {
    pub const ALL: [Self; 6] = [
        Self::Glass,
        Self::Drywall,
        Self::Wood,
        Self::Brick,
        Self::Concrete,
        Self::Metal,
    ];

    /// What a signal on `band` loses crossing the wall once, in dB: typical values for an
    /// interior wall (plain glass, a stud wall with gypsum boards, a solid wood wall or door,
    /// a brick wall, a reinforced concrete wall, a metal one), rising with the frequency.
    pub const fn loss_db(self, band: Band) -> f64 {
        let (g2, g5, g6) = match self {
            Self::Glass => (2.0, 4.0, 5.0),
            Self::Drywall => (3.0, 4.0, 5.0),
            Self::Wood => (4.0, 6.0, 7.0),
            Self::Brick => (8.0, 12.0, 14.0),
            Self::Concrete => (12.0, 18.0, 22.0),
            Self::Metal => (25.0, 32.0, 35.0),
        };
        match band {
            Band::G2 => g2,
            Band::G5 => g5,
            Band::G6 | Band::G60 => g6,
        }
    }
}

/// Why a floor's walls are refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WallError {
    #[error("A floor has at most {MAX_WALLS} walls")]
    TooMany,
    #[error("Walls are drawn on the floor's plan, which must be uploaded first")]
    NoPlan,
    #[error("A wall's ends lie on the plan, at least 1 px apart")]
    OffPlan,
}

/// Checks `walls` against the floor's plan.
pub fn validate_walls(walls: &[Wall], image: Option<&FloorImage>) -> Result<(), WallError> {
    if walls.len() > MAX_WALLS {
        return Err(WallError::TooMany);
    }
    if walls.is_empty() {
        return Ok(());
    }
    let image = image.ok_or(WallError::NoPlan)?;
    let fits = |wall: &Wall| {
        image.contains(wall.from)
            && image.contains(wall.to)
            && wall.from.distance(wall.to) >= MIN_WALL_PX
    };
    if walls.iter().all(fits) {
        Ok(())
    } else {
        Err(WallError::OffPlan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::floor::ImageKind;

    fn wall(x: f64) -> Wall {
        Wall {
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x, y: 10.0 },
            material: Material::Brick,
        }
    }

    #[test]
    fn walls_lie_on_the_plan() {
        let plan = FloorImage {
            kind: ImageKind::Png,
            width_px: 100,
            height_px: 100,
            bytes: 1,
            version: 1,
        };
        assert_eq!(validate_walls(&[], None), Ok(()));
        assert_eq!(validate_walls(&[wall(50.0)], Some(&plan)), Ok(()));
        assert_eq!(validate_walls(&[wall(50.0)], None), Err(WallError::NoPlan));
        assert_eq!(
            validate_walls(&[wall(101.0)], Some(&plan)),
            Err(WallError::OffPlan)
        );
        let dot = Wall {
            to: Point { x: 0.5, y: 0.5 },
            ..wall(1.0)
        };
        assert_eq!(validate_walls(&[dot], Some(&plan)), Err(WallError::OffPlan));
        let mut nan = wall(1.0);
        nan.to.x = f64::NAN;
        assert_eq!(validate_walls(&[nan], Some(&plan)), Err(WallError::OffPlan));
        assert_eq!(
            validate_walls(&vec![wall(50.0); MAX_WALLS + 1], Some(&plan)),
            Err(WallError::TooMany)
        );
    }

    #[test]
    fn losses_rise_with_the_band_and_the_material() {
        for band in [Band::G2, Band::G5, Band::G6] {
            let losses = Material::ALL.map(|m| m.loss_db(band));
            assert!(losses.windows(2).all(|w| w[0] <= w[1]), "{band:?}");
        }
        for material in Material::ALL {
            assert!(material.loss_db(Band::G2) < material.loss_db(Band::G5));
        }
        assert_eq!(
            serde_json::to_value(wall(1.0)).unwrap()["material"],
            "brick"
        );
    }
}

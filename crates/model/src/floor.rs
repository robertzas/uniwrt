//! Floor plans (features/wifi.md, Coverage & floor plans; D-117): each floor's image, its scale
//! and where nodes are placed on it. Not part of the site's desired state: nothing here goes to a
//! node, so edits save at once instead of through review and apply.
//!
//! Positions are in the image's pixels, from its top-left corner. The scale is a reference line
//! drawn on the image with its real length, kept as drawn so the editor can show it again.

use serde::{Deserialize, Serialize};
use uniwrt_core::{FloorId, NodeId, Timestamp};

/// The most floors a site keeps.
pub const MAX_FLOORS: usize = 32;
/// The longest side of a floor's image, in pixels: the browser scales larger plans down.
pub const MAX_IMAGE_SIDE: u32 = 4096;
/// The shortest side of a floor's image, in pixels.
pub const MIN_IMAGE_SIDE: u32 = 16;
/// The largest image accepted, in bytes.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// The longest floor name, in characters.
pub const MAX_NAME: usize = 64;
/// The storeys a floor's level may name, below and above the ground floor (0).
pub const LEVELS: std::ops::RangeInclusive<i16> = -9..=99;
/// From one floor to the next, in meters.
pub const HEIGHTS_M: std::ops::RangeInclusive<f64> = 2.0..=10.0;
/// The height a new floor gets, in meters.
pub const DEFAULT_HEIGHT_M: f64 = 3.0;
/// The real length a scale's line may have, in meters.
pub const SCALE_LENGTHS_M: std::ops::RangeInclusive<f64> = 0.1..=1000.0;
/// The shortest scale line, in pixels: shorter ones are too imprecise.
pub const MIN_SCALE_PX: f64 = 10.0;

/// One floor of the site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Floor {
    pub id: FloorId,
    pub name: String,
    /// The storey: 0 is the ground floor, negative ones are below it. Unique; floors are listed
    /// by level.
    pub level: i16,
    /// From this floor's floor to the next one's, in meters.
    pub height_m: f64,
    /// None until a plan is uploaded.
    pub image: Option<FloorImage>,
    /// None until it is set.
    pub scale: Option<FloorScale>,
    /// The nodes placed on this floor, by node id. A node is on one floor at most.
    pub nodes: Vec<Placement>,
    pub updated_at: Timestamp,
}

/// A floor's image as stored: the browser normalizes uploads to WebP (PNG where it can't
/// encode WebP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FloorImage {
    pub kind: ImageKind,
    pub width_px: u32,
    pub height_px: u32,
    pub bytes: u32,
    /// Grows with each upload: the image's URL carries it, so browsers cache each version for
    /// good.
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    Webp,
    Png,
}

impl ImageKind {
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Webp => "image/webp",
            Self::Png => "image/png",
        }
    }
}

/// A point on a floor's image, in pixels from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// A floor's scale: a line drawn on its image and the real length it spans.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FloorScale {
    pub from: Point,
    pub to: Point,
    pub length_m: f64,
    /// The unit the length was entered in, which the floor's distances are shown in.
    pub unit: LengthUnit,
}

impl FloorScale {
    /// Meters per pixel of the image.
    pub fn meters_per_px(&self) -> f64 {
        self.length_m / self.from.distance(self.to)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    M,
    Ft,
}

/// A node placed on a floor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Placement {
    pub node: NodeId,
    pub at: Point,
}

/// A floor's own fields, as a user edits them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct FloorFields {
    pub name: String,
    pub level: i16,
    pub height_m: f64,
    pub scale: Option<FloorScale>,
}

/// Why a floor's fields are refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FloorError {
    #[error("A floor's name is 1 to {MAX_NAME} characters")]
    Name,
    #[error("A floor's level is from -9 to 99 (0 is the ground floor)")]
    Level,
    #[error("A floor is 2 to 10 m high")]
    Height,
    #[error("A scale's line is at least 10 px long and spans 0.1 to 1000 m")]
    Scale,
    #[error("A scale needs the floor's plan, and its line must lie on it")]
    ScaleOffPlan,
}

impl FloorFields {
    /// Trims the name and checks every field; a scale must lie on `image` when there is one.
    pub fn validate(mut self, image: Option<&FloorImage>) -> Result<Self, FloorError> {
        self.name = self.name.trim().to_owned();
        let chars = self.name.chars().count();
        if chars == 0 || chars > MAX_NAME || self.name.chars().any(char::is_control) {
            return Err(FloorError::Name);
        }
        if !LEVELS.contains(&self.level) {
            return Err(FloorError::Level);
        }
        if !HEIGHTS_M.contains(&self.height_m) {
            return Err(FloorError::Height);
        }
        if let Some(scale) = &self.scale {
            let span = scale.from.distance(scale.to);
            if !span.is_finite()
                || span < MIN_SCALE_PX
                || !SCALE_LENGTHS_M.contains(&scale.length_m)
            {
                return Err(FloorError::Scale);
            }
            let image = image.ok_or(FloorError::ScaleOffPlan)?;
            if !(image.contains(scale.from) && image.contains(scale.to)) {
                return Err(FloorError::ScaleOffPlan);
            }
        }
        Ok(self)
    }
}

impl FloorImage {
    /// Whether the point lies on the image, edges included.
    pub fn contains(&self, point: Point) -> bool {
        (0.0..=f64::from(self.width_px)).contains(&point.x)
            && (0.0..=f64::from(self.height_px)).contains(&point.y)
    }

    /// Where `point` of an image of this size lands on one of `other`'s size: each axis
    /// stretched, as when the same drawing is uploaded again at another resolution.
    pub fn rescale(&self, point: Point, other: &Self) -> Point {
        Point {
            x: point.x * f64::from(other.width_px) / f64::from(self.width_px),
            y: point.y * f64::from(other.height_px) / f64::from(self.height_px),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width_px: u32, height_px: u32) -> FloorImage {
        FloorImage {
            kind: ImageKind::Webp,
            width_px,
            height_px,
            bytes: 1,
            version: 1,
        }
    }

    fn fields(scale: Option<FloorScale>) -> FloorFields {
        FloorFields {
            name: "  Ground  ".to_owned(),
            level: 0,
            height_m: 2.7,
            scale,
        }
    }

    fn scale(to_x: f64, length_m: f64) -> FloorScale {
        FloorScale {
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x: to_x, y: 0.0 },
            length_m,
            unit: LengthUnit::Ft,
        }
    }

    #[test]
    fn fields_are_trimmed_and_checked() {
        let plan = image(800, 600);
        assert_eq!(fields(None).validate(None).unwrap().name, "Ground");
        let ok = fields(Some(scale(400.0, 10.0)))
            .validate(Some(&plan))
            .unwrap();
        assert!((ok.scale.unwrap().meters_per_px() - 0.025).abs() < 1e-12);

        let refused = |fields: FloorFields, image| fields.validate(image).unwrap_err();
        let mut blank = fields(None);
        blank.name = " \t".to_owned();
        assert_eq!(refused(blank, None), FloorError::Name);
        let mut deep = fields(None);
        deep.level = -10;
        assert_eq!(refused(deep, None), FloorError::Level);
        let mut low = fields(None);
        low.height_m = f64::NAN;
        assert_eq!(refused(low, None), FloorError::Height);
        assert_eq!(
            refused(fields(Some(scale(5.0, 10.0))), Some(&plan)),
            FloorError::Scale
        );
        assert_eq!(
            refused(fields(Some(scale(400.0, 0.0))), Some(&plan)),
            FloorError::Scale
        );
        assert_eq!(
            refused(fields(Some(scale(400.0, 10.0))), None),
            FloorError::ScaleOffPlan
        );
        assert_eq!(
            refused(fields(Some(scale(801.0, 10.0))), Some(&plan)),
            FloorError::ScaleOffPlan
        );
    }

    #[test]
    fn points_follow_a_new_resolution_of_the_plan() {
        let point = image(800, 600).rescale(Point { x: 400.0, y: 150.0 }, &image(1600, 900));
        assert_eq!(point, Point { x: 800.0, y: 225.0 });
    }

    #[test]
    fn serde_names() {
        let floor = Floor {
            id: FloorId::new(1),
            name: "Ground".to_owned(),
            level: 0,
            height_m: 3.0,
            image: Some(image(800, 600)),
            scale: Some(scale(400.0, 10.0)),
            nodes: vec![Placement {
                node: NodeId::new(2),
                at: Point { x: 1.0, y: 2.0 },
            }],
            updated_at: Timestamp::from_unix_ms(5),
        };
        assert_eq!(
            serde_json::to_value(&floor).unwrap(),
            serde_json::json!({
                "id": 1, "name": "Ground", "level": 0, "height_m": 3.0,
                "image": { "kind": "webp", "width_px": 800, "height_px": 600, "bytes": 1, "version": 1 },
                "scale": { "from": { "x": 0.0, "y": 0.0 }, "to": { "x": 400.0, "y": 0.0 }, "length_m": 10.0, "unit": "ft" },
                "nodes": [{ "node": 2, "at": { "x": 1.0, "y": 2.0 } }],
                "updated_at": 5,
            })
        );
    }
}

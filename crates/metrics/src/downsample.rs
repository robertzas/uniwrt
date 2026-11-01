//! Reducing a series to a chart's width: Largest-Triangle-Three-Buckets (Steinarsson, 2013)
//! over each run of the series, where a run ends wherever the data stops for more than three
//! steps (the line breaks there, with a row of `None`s). Each kept point carries the envelope
//! (least minimum, greatest maximum) of the points it stands for.

use uniwrt_core::Timestamp;
use uniwrt_model::metric::{Point, Resolution};

/// A series as a chart draws it, in columns; a row of `None`s breaks the line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Downsampled {
    pub at: Vec<Timestamp>,
    pub avg: Vec<Option<f64>>,
    pub min: Vec<Option<f64>>,
    pub max: Vec<Option<f64>>,
}

impl Downsampled {
    fn push(&mut self, point: Point) {
        self.at.push(point.at);
        self.avg.push(Some(round(point.avg)));
        self.min.push(Some(round(point.min)));
        self.max.push(Some(round(point.max)));
    }

    fn gap(&mut self, at: Timestamp) {
        self.at.push(at);
        self.avg.push(None);
        self.min.push(None);
        self.max.push(None);
    }
}

/// `points`, oldest first and read at `resolution`, reduced to about `width` points.
pub fn downsample(points: &[Point], resolution: Resolution, width: usize) -> Downsampled {
    let max_gap = 3 * resolution.step().as_millis();
    let mut runs = Vec::new();
    let mut start = 0;
    for i in 1..points.len() {
        let gap = points[i].at.as_unix_ms() - points[i - 1].at.as_unix_ms();
        if u128::try_from(gap).unwrap_or(0) > max_gap {
            runs.push(&points[start..i]);
            start = i;
        }
    }
    if start < points.len() {
        runs.push(&points[start..]);
    }
    let width = width.max(2);
    let mut out = Downsampled::default();
    for (index, run) in runs.iter().enumerate() {
        if index > 0 {
            let before = runs[index - 1][runs[index - 1].len() - 1].at.as_unix_ms();
            let after = run[0].at.as_unix_ms();
            out.gap(Timestamp::from_unix_ms(before.midpoint(after)));
        }
        // Each run's share of the width, at least its ends.
        let share = (width * run.len()).div_ceil(points.len()).max(2);
        lttb(run, share, &mut out);
    }
    out
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bucket bounds: non-negative and within the slice"
)]
fn lttb(points: &[Point], threshold: usize, out: &mut Downsampled) {
    if points.len() <= threshold {
        for &point in points {
            out.push(point);
        }
        return;
    }
    let x = |point: &Point| point.at.as_unix_ms() as f64;
    let last = points.len() - 1;
    // The points between the ends fall into `threshold - 2` buckets of equal size.
    let size = (points.len() - 2) as f64 / (threshold - 2) as f64;
    let bounds = |bucket: usize| {
        let from = (bucket as f64 * size) as usize + 1;
        let to = (((bucket + 1) as f64 * size) as usize + 1).min(last);
        from..to.max(from + 1)
    };
    out.push(points[0]);
    let mut previous = points[0];
    for bucket in 0..threshold - 2 {
        let range = bounds(bucket);
        // The average of the next bucket (the last point, after the last bucket).
        let next = if bucket + 1 < threshold - 2 {
            &points[bounds(bucket + 1)]
        } else {
            &points[last..]
        };
        let n = next.len() as f64;
        let (next_x, next_y) = (
            next.iter().map(x).sum::<f64>() / n,
            next.iter().map(|point| point.avg).sum::<f64>() / n,
        );
        let mut best = range.start;
        let mut best_area = f64::NEG_INFINITY;
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in range {
            let point = &points[i];
            min = min.min(point.min);
            max = max.max(point.max);
            let area = ((x(&previous) - next_x) * (point.avg - previous.avg)
                - (x(&previous) - x(point)) * (next_y - previous.avg))
                .abs();
            if area > best_area {
                best_area = area;
                best = i;
            }
        }
        let kept = Point {
            min,
            max,
            ..points[best]
        };
        out.push(kept);
        previous = points[best];
    }
    out.push(points[last]);
}

/// Four significant digits: what a chart can show, in a fraction of the JSON.
#[expect(clippy::cast_possible_truncation, reason = "a decimal exponent")]
fn round(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let digits = 3 - value.abs().log10().floor() as i32;
    if digits >= 0 {
        let scale = 10f64.powi(digits);
        (value * scale).round() / scale
    } else {
        let scale = 10f64.powi(-digits);
        (value / scale).round() * scale
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use uniwrt_core::Timestamp;
    use uniwrt_model::metric::{Point, Resolution};

    use super::{downsample, round};

    fn series(values: &[f64], every_ms: i64) -> Vec<Point> {
        values
            .iter()
            .enumerate()
            .map(|(i, &value)| {
                Point::raw(
                    Timestamp::from_unix_ms(i64::try_from(i).unwrap() * every_ms),
                    value,
                )
            })
            .collect()
    }

    #[test]
    fn short_series_pass_through() {
        let points = series(&[1.0, 2.0, 3.0], 15_000);
        let out = downsample(&points, Resolution::Raw, 100);
        assert_eq!(out.avg, [Some(1.0), Some(2.0), Some(3.0)]);
        assert_eq!(out.at.len(), 3);
    }

    #[test]
    fn peaks_survive_and_envelopes_cover_what_was_dropped() {
        let mut values = vec![10.0; 1_000];
        values[500] = 90.0;
        values[250] = 1.0;
        let points = series(&values, 15_000);
        let out = downsample(&points, Resolution::Raw, 50);
        assert!(out.at.len() <= 50, "{}", out.at.len());
        assert!(out.avg.contains(&Some(90.0)));
        assert_eq!(
            out.min
                .iter()
                .flatten()
                .copied()
                .fold(f64::INFINITY, f64::min),
            1.0
        );
        assert_eq!(out.at.first(), points.first().map(|point| &point.at));
        assert_eq!(out.at.last(), points.last().map(|point| &point.at));
    }

    #[test]
    fn the_line_breaks_where_the_data_stops() {
        let mut points = series(&[1.0, 2.0, 3.0], 15_000);
        // Ten minutes without data, then two more windows.
        let later = |ms: i64, value| Point::raw(Timestamp::from_unix_ms(630_000 + ms), value);
        points.extend([later(0, 4.0), later(15_000, 5.0)]);
        let out = downsample(&points, Resolution::Raw, 100);
        assert_eq!(
            out.avg,
            [Some(1.0), Some(2.0), Some(3.0), None, Some(4.0), Some(5.0)]
        );
        assert_eq!(out.at[3], Timestamp::from_unix_ms(330_000));
        // Missing a window or two is not a gap.
        let points = series(&[1.0, 2.0], 45_000);
        assert_eq!(downsample(&points, Resolution::Raw, 100).avg.len(), 2);
    }

    #[test]
    fn values_keep_four_significant_digits() {
        assert_eq!(round(23.333_333), 23.33);
        assert_eq!(round(-92.0), -92.0);
        assert_eq!(round(123_456_789.0), 123_500_000.0);
        assert_eq!(round(0.012_345), 0.012_35);
        assert_eq!(round(0.0), 0.0);
    }

    proptest! {
        #[test]
        fn output_stays_within_the_width_and_in_order(
            values in prop::collection::vec(-1e6f64..1e6, 0..2_000),
            width in 2usize..500,
        ) {
            let points = series(&values, 15_000);
            let out = downsample(&points, Resolution::Raw, width);
            prop_assert!(out.at.len() <= width.max(2) + 1);
            prop_assert!(out.at.windows(2).all(|pair| pair[0] < pair[1]));
            prop_assert_eq!(out.at.len(), out.avg.len());
            for ((min, avg), max) in out.min.iter().zip(&out.avg).zip(&out.max) {
                if let (Some(min), Some(avg), Some(max)) = (min, avg, max) {
                    prop_assert!(min <= avg && avg <= max);
                }
            }
        }
    }
}

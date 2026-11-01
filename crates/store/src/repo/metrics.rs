//! [`MetricsRepo`] on SQLite: the `metric_series`, `metrics_raw`, `metrics_1m`, `metrics_1h`
//! and `metric_rollups` tables.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use uniwrt_core::Timestamp;
use uniwrt_model::metric::{Entity, Metric, Point, Resolution, Sample};

use super::{MetricsPrune, MetricsRepo};
use crate::{Result, Store};

/// The most buckets one call rolls up: an hour of minutes, a day of hours. A long catch-up
/// takes several calls, so the writer is never held for long.
const MINUTES_PER_CALL: i64 = 60;
const HOURS_PER_CALL: i64 = 24;
const MINUTE: i64 = step_ms(Resolution::Minute);
const HOUR: i64 = step_ms(Resolution::Hour);

/// What `ts` and the IN list name in each statement: every series, so SQLite walks the
/// primary key one series at a time instead of scanning the table.
const EVERY_SERIES: &str = "series IN (SELECT id FROM metric_series)";

impl MetricsRepo for Store {
    async fn record_metrics(&self, samples: Vec<Sample>) -> Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        self.write(move |conn| {
            let mut series = conn.prepare_cached(
                "INSERT INTO metric_series (entity, metric) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            )?;
            let mut insert = conn.prepare_cached(
                "INSERT INTO metrics_raw (series, ts, value)
                 SELECT id, ?3, ?4 FROM metric_series WHERE entity = ?1 AND metric = ?2
                 ON CONFLICT (series, ts) DO UPDATE SET value = excluded.value",
            )?;
            for sample in &samples {
                let entity = sample.entity.to_string();
                series.execute(params![entity, sample.metric.name()])?;
                insert.execute(params![
                    entity,
                    sample.metric.name(),
                    sample.at.as_unix_ms(),
                    sample.value
                ])?;
            }
            Ok(())
        })
        .await
    }

    async fn metric_points(
        &self,
        entity: &Entity,
        metric: Metric,
        resolution: Resolution,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<Point>> {
        let entity = entity.to_string();
        self.read(move |conn| {
            let Some(series) = conn
                .prepare_cached("SELECT id FROM metric_series WHERE entity = ?1 AND metric = ?2")?
                .query_row(params![entity, metric.name()], |row| row.get::<_, i64>(0))
                .optional()?
            else {
                return Ok(Vec::new());
            };
            let mut points = Vec::new();
            let mut from = from.as_unix_ms();
            let to = to.as_unix_ms();
            let mut next = Some(resolution);
            // The resolution asked for, then its tail from finer ones, which rollups have not
            // reached yet.
            while let Some(resolution) = next {
                let found = points_at(conn, series, resolution, from, to)?;
                if let Some(last) = found.last() {
                    from = from.max(last.at.as_unix_ms() + step_ms(resolution));
                }
                points.extend(found);
                next = resolution.finer();
            }
            Ok(points)
        })
        .await
    }

    async fn metric_series(&self, under: Option<Entity>) -> Result<Vec<(Entity, Vec<Metric>)>> {
        let under = under.map(|entity| entity.to_string());
        self.read(move |conn| {
            let mut statement = conn.prepare_cached(
                "SELECT entity, metric FROM metric_series
                 WHERE ?1 IS NULL OR entity = ?1 OR substr(entity, 1, length(?1) + 1) = ?1 || '/'
                 ORDER BY entity, metric",
            )?;
            let rows = statement.query_map([under], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut series = BTreeMap::<Entity, Vec<Metric>>::new();
            for row in rows {
                let (entity, metric) = row?;
                // Names a newer uniwrt wrote are skipped.
                if let (Ok(entity), Ok(metric)) = (entity.parse(), metric.parse()) {
                    series.entry(entity).or_default().push(metric);
                }
            }
            Ok(series
                .into_iter()
                .map(|(entity, mut metrics)| {
                    metrics.sort();
                    (entity, metrics)
                })
                .collect())
        })
        .await
    }

    async fn roll_up_metrics(&self, until: Timestamp, earliest: Timestamp) -> Result<bool> {
        self.write(move |conn| {
            let (until, earliest) = (until.as_unix_ms(), earliest.as_unix_ms());
            let minutes_due = floor(until, MINUTE);
            let minutes_from = done_until(conn, "minute")?.unwrap_or(floor(earliest, MINUTE));
            let minutes_to = minutes_due.min(minutes_from + MINUTES_PER_CALL * MINUTE);
            if minutes_to > minutes_from {
                roll_up(conn, Resolution::Minute, minutes_from, minutes_to)?;
            }
            let minutes_done = minutes_to.max(minutes_from);
            let hours_due = floor(minutes_done, HOUR);
            let hours_from = done_until(conn, "hour")?.unwrap_or(floor(earliest, HOUR));
            let hours_to = hours_due.min(hours_from + HOURS_PER_CALL * HOUR);
            if hours_to > hours_from {
                roll_up(conn, Resolution::Hour, hours_from, hours_to)?;
            }
            Ok(minutes_to < minutes_due || hours_to < hours_due)
        })
        .await
    }

    async fn prune_metrics(&self, before: MetricsPrune) -> Result<usize> {
        self.write(move |conn| {
            let mut deleted = 0;
            for (table, before) in [
                ("metrics_raw", before.raw),
                ("metrics_1m", before.minute),
                ("metrics_1h", before.hour),
            ] {
                deleted += conn
                    .prepare_cached(&format!(
                        "DELETE FROM {table} WHERE {EVERY_SERIES} AND ts < ?1"
                    ))?
                    .execute([before.as_unix_ms()])?;
            }
            conn.prepare_cached(
                "DELETE FROM metric_series WHERE
                 NOT EXISTS (SELECT 1 FROM metrics_raw WHERE series = id) AND
                 NOT EXISTS (SELECT 1 FROM metrics_1m WHERE series = id) AND
                 NOT EXISTS (SELECT 1 FROM metrics_1h WHERE series = id)",
            )?
            .execute([])?;
            Ok(deleted)
        })
        .await
    }
}

fn points_at(
    conn: &Connection,
    series: i64,
    resolution: Resolution,
    from: i64,
    to: i64,
) -> Result<Vec<Point>> {
    let sql = match resolution {
        Resolution::Raw => {
            "SELECT ts, value, value, value FROM metrics_raw
             WHERE series = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts"
        }
        Resolution::Minute => {
            "SELECT ts, low, mean, high FROM metrics_1m
             WHERE series = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts"
        }
        Resolution::Hour => {
            "SELECT ts, low, mean, high FROM metrics_1h
             WHERE series = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts"
        }
    };
    Ok(conn
        .prepare_cached(sql)?
        .query_map(params![series, from, to], |row| {
            Ok(Point {
                at: Timestamp::from_unix_ms(row.get(0)?),
                min: row.get(1)?,
                avg: row.get(2)?,
                max: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Rolls the finer resolution up into `into`'s buckets from `from` to `to`, and records how far
/// it got. Buckets are recomputed whole, so rolling one up again is harmless.
fn roll_up(conn: &Connection, into: Resolution, from: i64, to: i64) -> Result<()> {
    let (table, source, size, name) = match into {
        Resolution::Minute => ("metrics_1m", "metrics_raw", MINUTE, "minute"),
        Resolution::Hour => ("metrics_1h", "metrics_1m", HOUR, "hour"),
        Resolution::Raw => return Ok(()),
    };
    // Raw windows are single values; minutes carry their own envelope and weight.
    let (low, mean, high, last, n) = match into {
        Resolution::Minute => ("value", "value", "value", "value", "1"),
        _ => ("low", "mean", "high", "last", "n"),
    };
    conn.prepare_cached(&format!(
        "INSERT INTO {table} (series, ts, low, mean, high, last, n)
         SELECT series, bucket, min(low), sum(mean * n) / sum(n), max(high), min(last), sum(n)
         FROM (
             SELECT series, ts - ts % {size} AS bucket, {low} AS low, {mean} AS mean,
                 {high} AS high, {n} AS n,
                 last_value({last}) OVER (
                     PARTITION BY series, ts - ts % {size} ORDER BY ts
                     ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
                 ) AS last
             FROM {source} WHERE {EVERY_SERIES} AND ts >= ?1 AND ts < ?2
         )
         WHERE true GROUP BY series, bucket
         ON CONFLICT (series, ts) DO UPDATE SET low = excluded.low, mean = excluded.mean,
         high = excluded.high, last = excluded.last, n = excluded.n"
    ))?
    .execute([from, to])?;
    conn.prepare_cached(
        "INSERT INTO metric_rollups (resolution, done_until) VALUES (?1, ?2)
         ON CONFLICT (resolution) DO UPDATE SET done_until = excluded.done_until",
    )?
    .execute(params![name, to])?;
    Ok(())
}

fn done_until(conn: &Connection, resolution: &str) -> Result<Option<i64>> {
    Ok(conn
        .prepare_cached("SELECT done_until FROM metric_rollups WHERE resolution = ?1")?
        .query_row([resolution], |row| row.get(0))
        .optional()?)
}

fn floor(ms: i64, size: i64) -> i64 {
    ms - ms.rem_euclid(size)
}

#[expect(clippy::cast_possible_truncation, reason = "an hour at most")]
const fn step_ms(resolution: Resolution) -> i64 {
    resolution.step().as_millis() as i64
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;

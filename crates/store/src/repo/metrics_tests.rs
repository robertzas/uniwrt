//! Metrics over a real database: recording, rollups with their envelopes and weights, reading
//! with tails from finer resolutions, the series catalogue, and retention.

use std::time::Duration;

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::metric::{Entity, Metric, Point, Resolution, Sample};

use crate::{
    MetricsPrune, MetricsRepo as _,
    testing::{START, open},
};

const NODE: NodeId = NodeId::new(1);

fn at(secs: u64) -> Timestamp {
    START.saturating_add(Duration::from_secs(secs))
}

fn sample(entity: &Entity, metric: Metric, secs: u64, value: f64) -> Sample {
    Sample {
        entity: entity.clone(),
        metric,
        at: at(secs),
        value,
    }
}

fn avgs(points: &[Point]) -> Vec<f64> {
    points.iter().map(|point| point.avg).collect()
}

#[tokio::test]
async fn samples_roll_up_into_minutes_and_hours() {
    let (_dir, store, _clock) = open().await;
    let node = Entity::Node(NODE);
    // Two hours of windows: 10 in the first four of every minute, then 40 at its last.
    let mut samples = Vec::new();
    for minute in 0..120 {
        for (window, value) in [(0, 10.0), (15, 10.0), (30, 10.0), (45, 40.0)] {
            samples.push(sample(&node, Metric::Cpu, minute * 60 + window, value));
        }
    }
    store.record_metrics(samples).await.unwrap();
    // A sample again replaces its window's value.
    store
        .record_metrics(vec![sample(&node, Metric::Cpu, 0, 70.0)])
        .await
        .unwrap();

    // The first call rolls up an hour of minutes, then the hour they complete.
    assert!(store.roll_up_metrics(at(7_230), START).await.unwrap());
    assert!(!store.roll_up_metrics(at(7_230), START).await.unwrap());
    let minutes = store
        .metric_points(&node, Metric::Cpu, Resolution::Minute, START, at(7_200))
        .await
        .unwrap();
    assert_eq!(minutes.len(), 120);
    assert_eq!(
        minutes[0],
        Point {
            at: START,
            min: 10.0,
            avg: 32.5,
            max: 70.0
        }
    );
    assert_eq!(
        minutes[1],
        Point {
            at: at(60),
            min: 10.0,
            avg: 17.5,
            max: 40.0
        }
    );
    let hours = store
        .metric_points(&node, Metric::Cpu, Resolution::Hour, START, at(7_200))
        .await
        .unwrap();
    assert_eq!(hours.len(), 2);
    // Minutes are weighed by their windows: (32.5 + 59 × 17.5) / 60.
    assert_eq!(
        hours[1],
        Point {
            at: at(3_600),
            min: 10.0,
            avg: 17.5,
            max: 40.0
        }
    );
    assert!((hours[0].avg - 17.75).abs() < 1e-9, "{}", hours[0].avg);
    assert_eq!(hours[0].max, 70.0);
}

#[tokio::test]
async fn reads_fill_the_tail_from_finer_resolutions() {
    let (_dir, store, _clock) = open().await;
    let radio = Entity::Radio {
        node: NODE,
        name: "radio0".to_owned(),
    };
    let samples = (0..20)
        .map(|i: u32| sample(&radio, Metric::Busy, u64::from(i) * 15, f64::from(i)))
        .collect();
    store.record_metrics(samples).await.unwrap();
    // Minutes up to 3:00 are rolled up; the windows after are raw.
    store.roll_up_metrics(at(200), START).await.unwrap();
    let points = store
        .metric_points(&radio, Metric::Busy, Resolution::Minute, START, at(300))
        .await
        .unwrap();
    let at_secs: Vec<_> = points.iter().map(|point| point.at).collect();
    assert_eq!(
        at_secs,
        [
            at(0),
            at(60),
            at(120),
            at(180),
            at(195),
            at(210),
            at(225),
            at(240),
            at(255),
            at(270),
            at(285)
        ]
    );
    assert_eq!(avgs(&points)[..3], [1.5, 5.5, 9.5]);
    assert_eq!(points[3].avg, 12.0);
    // Nothing for an unknown series.
    let none = store
        .metric_points(&radio, Metric::Noise, Resolution::Raw, START, at(300))
        .await
        .unwrap();
    assert_eq!(none, []);
}

#[tokio::test]
async fn the_catalogue_lists_series_by_entity() {
    let (_dir, store, _clock) = open().await;
    let device = Entity::Device {
        node: NODE,
        name: "br-lan".to_owned(),
    };
    let other = Entity::Node(NodeId::new(12));
    store
        .record_metrics(vec![
            sample(&Entity::Node(NODE), Metric::Memory, 0, 1.0),
            sample(&Entity::Node(NODE), Metric::Cpu, 0, 1.0),
            sample(&device, Metric::RxBps, 0, 1.0),
            sample(&other, Metric::Cpu, 0, 1.0),
            sample(&Entity::Site, Metric::Clients, 0, 1.0),
        ])
        .await
        .unwrap();
    let under = store.metric_series(Some(Entity::Node(NODE))).await.unwrap();
    assert_eq!(
        under,
        [
            (Entity::Node(NODE), vec![Metric::Cpu, Metric::Memory]),
            (device, vec![Metric::RxBps])
        ]
    );
    assert_eq!(store.metric_series(None).await.unwrap().len(), 4);
}

#[tokio::test]
async fn retention_deletes_old_points_and_empty_series() {
    let (_dir, store, _clock) = open().await;
    let old = Entity::Client("f0:18:98:00:00:01".parse().unwrap());
    let node = Entity::Node(NODE);
    store
        .record_metrics(vec![
            sample(&old, Metric::Signal, 0, -60.0),
            sample(&node, Metric::Cpu, 0, 5.0),
            sample(&node, Metric::Cpu, 7_200, 6.0),
        ])
        .await
        .unwrap();
    store.roll_up_metrics(at(3_700), START).await.unwrap();
    let deleted = store
        .prune_metrics(MetricsPrune {
            raw: at(3_600),
            minute: at(3_600),
            hour: START,
        })
        .await
        .unwrap();
    // Two raw windows and two minutes.
    assert_eq!(deleted, 4);
    let left = store.metric_series(None).await.unwrap();
    // The client's hour of rollup keeps its series.
    assert_eq!(left.len(), 2);
    store
        .prune_metrics(MetricsPrune {
            raw: at(3_600),
            minute: at(3_600),
            hour: at(3_600),
        })
        .await
        .unwrap();
    assert_eq!(
        store.metric_series(None).await.unwrap(),
        [(node, vec![Metric::Cpu])]
    );
}

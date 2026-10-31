//! The history's frames: the policy over hand-made topologies, then the recorder over a store.

use std::{sync::Arc, time::Duration};

use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_store::{Store, TopologyFrame, TopologyRepo as _};
use uniwrt_topo::{Medium, TopoEdge, TopoKind, TopoNode, Topology, Traffic};

use super::{Frames, HistoryConfig, TopologyHistory, replay};
use crate::{Engine, EngineConfig, Event, testing::SharedLink};

const HOUR: Duration = Duration::from_hours(1);

fn vertex(id: &str, kind: TopoKind) -> TopoNode {
    TopoNode {
        id: id.to_owned(),
        kind,
        node: None,
        mac: None,
        name: None,
        ips: Vec::new(),
        state: None,
        cpu_percent: Some(12.0),
        memory_percent: Some(40.0),
        parent: None,
        uplink: None,
        hops: None,
    }
}

/// The internet, the gateway and an access point at `signal` dBm of it, at `ms`, with `bytes`
/// carried over the mesh link.
fn house(ms: i64, signal: i16, bytes: u64) -> Topology {
    let mesh = TopoEdge {
        id: "node:1~node:2~mesh~5g".to_owned(),
        a: "node:1".to_owned(),
        b: "node:2".to_owned(),
        medium: Medium::Mesh,
        a_port: None,
        b_port: None,
        up: true,
        band: None,
        ssid: None,
        signal_dbm: Some(signal),
        tq: None,
        capacity_mbps: None,
        quality: Some(50),
        traffic: Some(Traffic {
            at: Timestamp::from_unix_ms(ms),
            down_bytes: bytes,
            up_bytes: bytes,
            down_bps: None,
            up_bps: None,
        }),
    };
    Topology {
        at: Timestamp::from_unix_ms(ms),
        nodes: vec![
            vertex("internet", TopoKind::Internet),
            vertex("node:1", TopoKind::Gateway),
            vertex("node:2", TopoKind::Ap),
        ],
        edges: vec![mesh],
    }
}

fn record(frames: &mut Frames, topology: &Topology) -> Option<TopologyFrame> {
    let (frame, kept) = frames.next(topology).unwrap()?;
    frames.recorded(kept);
    Some(frame)
}

#[test]
fn a_keyframe_comes_first_and_every_hour_with_deltas_of_what_shows_between() {
    let mut frames = Frames::new(HOUR);
    let first = record(&mut frames, &house(1_000, -62, 10)).unwrap();
    assert!(first.keyframe);
    assert_eq!(
        record(&mut frames, &house(2_000, -61, 99)),
        None,
        "noise and traffic"
    );
    let delta = record(&mut frames, &house(3_000, -70, 99)).unwrap();
    assert!(!delta.keyframe);
    assert!(delta.data.contains("-70") && !delta.data.contains("node:1\",\"kind"));
    let hour = 1_000 + 3_600_000;
    assert!(record(&mut frames, &house(hour, -70, 99)).unwrap().keyframe);
    assert!(
        record(&mut frames, &house(500, -70, 99)).unwrap().keyframe,
        "the clock went back"
    );

    let replayed = replay(&[first, delta]).unwrap().unwrap();
    assert_eq!(replayed, house(3_000, -70, 0).for_history());
}

#[test]
fn a_replay_needs_a_keyframe_first() {
    assert_eq!(replay(&[]).unwrap(), None);
    let delta = TopologyFrame {
        at: Timestamp::from_unix_ms(1),
        keyframe: false,
        data: "{}".to_owned(),
    };
    assert_eq!(replay(&[delta]).unwrap(), None);
}

#[tokio::test]
async fn the_recorder_coalesces_changes_and_prunes_at_each_keyframe() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_secs(1_793_200_000)));
    let store = Store::open(dir.path().join("uniwrt.db"), clock.clone())
        .await
        .unwrap();
    let engine = Engine::<SharedLink>::new(EngineConfig::default(), clock);
    let config = HistoryConfig {
        min_interval: Duration::from_millis(100),
        keyframe_every: HOUR,
        keep: HOUR,
    };
    let history = TopologyHistory::spawn(&engine, store.clone(), config);
    let publish = |topology: Topology| {
        engine.context().publish(Event::Topology {
            topology: Arc::new(topology),
        });
    };
    let settle = || tokio::time::sleep(Duration::from_millis(300));
    publish(house(1_000, -60, 0));
    publish(house(2_000, -66, 0));
    publish(house(3_000, -70, 0));
    settle().await;
    publish(house(4_000, -70, 5));
    settle().await;
    let times = |store: Store| async move {
        let times = store.topology_times().await.unwrap();
        times
            .into_iter()
            .map(|(at, keyframe)| (at.as_unix_ms(), keyframe))
            .collect::<Vec<_>>()
    };
    assert_eq!(times(store.clone()).await, [(1_000, true), (3_000, false)]);
    let frames = store
        .topology_frames(Timestamp::from_unix_ms(3_500))
        .await
        .unwrap();
    assert_eq!(
        replay(&frames).unwrap(),
        Some(house(3_000, -70, 0).for_history())
    );

    // Two hours on, then four: the second keyframe prunes all before the first of them.
    let later = |hours: i64| 1_000 + hours * 3_600_000;
    publish(house(later(2), -70, 0));
    settle().await;
    publish(house(later(4), -60, 0));
    history.stop().await;
    assert_eq!(
        times(store.clone()).await,
        [(later(2), true), (later(4), true)]
    );
    engine.shutdown().await;
    store.close().await;
}

//! [`TopologyRepo`] on SQLite: the `topology_history` and `topology_layout` tables.

use rusqlite::{OptionalExtension as _, params};
use uniwrt_core::Timestamp;

use super::{Pin, TopologyFrame, TopologyRepo};
use crate::{Result, Store, codec::timestamp_column};

/// The newest keyframe at or before `?1`: where the topology at that time starts.
const KEYFRAME_BEFORE: &str = "SELECT id FROM topology_history WHERE keyframe = 1 AND at <= ?1
     ORDER BY at DESC, id DESC LIMIT 1";

impl TopologyRepo for Store {
    async fn record_topology(&self, frame: TopologyFrame) -> Result<()> {
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO topology_history (at, keyframe, data) VALUES (?1, ?2, ?3)",
            )?
            .execute(params![frame.at.as_unix_ms(), frame.keyframe, frame.data])?;
            Ok(())
        })
        .await
    }

    async fn topology_frames(&self, at: Timestamp) -> Result<Vec<TopologyFrame>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT at, keyframe, data FROM topology_history
                     WHERE id >= ({KEYFRAME_BEFORE}) AND at <= ?1 ORDER BY id"
                ))?
                .query_map([at.as_unix_ms()], |row| {
                    Ok(TopologyFrame {
                        at: timestamp_column(row, 0)?,
                        keyframe: row.get(1)?,
                        data: row.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn topology_times(&self) -> Result<Vec<(Timestamp, bool)>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached("SELECT at, keyframe FROM topology_history ORDER BY id")?
                .query_map([], |row| Ok((timestamp_column(row, 0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn prune_topology(&self, before: Timestamp) -> Result<usize> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "DELETE FROM topology_history WHERE id < ({KEYFRAME_BEFORE})"
                ))?
                .execute([before.as_unix_ms()])?)
        })
        .await
    }

    async fn pins(&self) -> Result<Vec<Pin>> {
        self.read(|conn| {
            Ok(conn
                .prepare_cached("SELECT vertex, x, y FROM topology_layout ORDER BY vertex")?
                .query_map([], |row| {
                    Ok(Pin {
                        vertex: row.get(0)?,
                        x: row.get(1)?,
                        y: row.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn pin(&self, pin: Pin, max: usize) -> Result<bool> {
        let max = i64::try_from(max).unwrap_or(i64::MAX);
        self.write(move |conn| {
            let pinned = conn
                .prepare_cached("SELECT 1 FROM topology_layout WHERE vertex = ?1")?
                .query_row([&pin.vertex], |_| Ok(()))
                .optional()?
                .is_some();
            let count: i64 = conn
                .prepare_cached("SELECT count(*) FROM topology_layout")?
                .query_row([], |row| row.get(0))?;
            if !pinned && count >= max {
                return Ok(false);
            }
            conn.prepare_cached(
                "INSERT INTO topology_layout (vertex, x, y) VALUES (?1, ?2, ?3)
                 ON CONFLICT (vertex) DO UPDATE SET x = excluded.x, y = excluded.y",
            )?
            .execute(params![pin.vertex, pin.x, pin.y])?;
            Ok(true)
        })
        .await
    }

    async fn unpin(&self, vertex: &str) -> Result<bool> {
        let vertex = vertex.to_owned();
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM topology_layout WHERE vertex = ?1")?
                .execute([vertex])?
                > 0)
        })
        .await
    }

    async fn unpin_all(&self) -> Result<usize> {
        self.write(|conn| Ok(conn.execute("DELETE FROM topology_layout", [])?))
            .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;

    use crate::{Pin, TopologyFrame, TopologyRepo as _, testing};

    fn frame(ms: i64, keyframe: bool) -> TopologyFrame {
        TopologyFrame {
            at: Timestamp::from_unix_ms(ms),
            keyframe,
            data: format!("{ms}"),
        }
    }

    fn data(frames: &[TopologyFrame]) -> Vec<&str> {
        frames.iter().map(|frame| frame.data.as_str()).collect()
    }

    #[tokio::test]
    async fn the_topology_at_a_time_starts_at_the_keyframe_before_it_and_pruning_keeps_it() {
        let (_dir, store, _) = testing::open().await;
        let none: [TopologyFrame; 0] = [];
        assert_eq!(
            store
                .topology_frames(Timestamp::from_unix_ms(5))
                .await
                .unwrap(),
            none
        );
        for (ms, keyframe) in [
            (10, true),
            (20, false),
            (30, false),
            (40, true),
            (50, false),
        ] {
            store.record_topology(frame(ms, keyframe)).await.unwrap();
        }
        let at = |ms| store.topology_frames(Timestamp::from_unix_ms(ms));
        assert!(at(9).await.unwrap().is_empty(), "before the first keyframe");
        assert_eq!(data(&at(25).await.unwrap()), ["10", "20"]);
        assert_eq!(data(&at(40).await.unwrap()), ["40"]);
        assert_eq!(data(&at(99).await.unwrap()), ["40", "50"]);
        assert_eq!(
            store.topology_times().await.unwrap()[..2],
            [
                (Timestamp::from_unix_ms(10), true),
                (Timestamp::from_unix_ms(20), false)
            ]
        );

        assert_eq!(
            store
                .prune_topology(Timestamp::from_unix_ms(5))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .prune_topology(Timestamp::from_unix_ms(45))
                .await
                .unwrap(),
            3
        );
        assert_eq!(data(&at(45).await.unwrap()), ["40"]);
        assert_eq!(store.topology_times().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn pins_move_and_go_and_a_full_layout_takes_no_new_ones() {
        let (_dir, store, _) = testing::open().await;
        let pin = |vertex: &str, x| Pin {
            vertex: vertex.to_owned(),
            x,
            y: -x,
        };
        assert!(store.pin(pin("node:1", 1.0), 2).await.unwrap());
        assert!(store.pin(pin("node:2", 2.0), 2).await.unwrap());
        assert!(!store.pin(pin("node:3", 3.0), 2).await.unwrap(), "full");
        assert!(store.pin(pin("node:1", 5.5), 2).await.unwrap(), "moved");
        assert_eq!(
            store.pins().await.unwrap(),
            [pin("node:1", 5.5), pin("node:2", 2.0)]
        );
        assert!(store.unpin("node:2").await.unwrap());
        assert!(!store.unpin("node:2").await.unwrap());
        assert_eq!(store.unpin_all().await.unwrap(), 1);
        assert_eq!(store.pins().await.unwrap(), [] as [Pin; 0]);
    }
}

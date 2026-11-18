//! [`RolloutRepo`] on SQLite: the `rollouts` table, each rollout's nodes, canary, window and wait
//! as JSON.

use rusqlite::{OptionalExtension as _, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use uniwrt_core::{RolloutId, Timestamp, UserId};
use uniwrt_model::rollout::{NewRollout, Rollout, RolloutState};

use super::RolloutRepo;
use crate::{
    Error, Result, Store,
    codec::{enum_column, enum_text, timestamp_column},
};

const COLUMNS: &str =
    "id, state, nodes, canary, window, waiting, error, created_by, started_at, finished_at";

fn rollout_from_row(row: &Row<'_>) -> rusqlite::Result<Rollout> {
    Ok(Rollout {
        id: RolloutId::new(row.get(0)?),
        state: enum_column(row, 1)?,
        nodes: decode(2, &row.get::<_, String>(2)?)?,
        canary: optional(row, 3)?,
        window: optional(row, 4)?,
        waiting: optional(row, 5)?,
        error: row.get(6)?,
        created_by: row.get::<_, Option<i64>>(7)?.map(UserId::new),
        started_at: timestamp_column(row, 8)?,
        finished_at: row.get::<_, Option<i64>>(9)?.map(Timestamp::from_unix_ms),
    })
}

fn optional<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(index)?
        .map(|text| decode(index, &text))
        .transpose()
}

fn decode<T: DeserializeOwned>(index: usize, text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn encode(what: &'static str, value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|source| Error::Encode { what, source })
}

fn encode_opt<T: Serialize>(what: &'static str, value: Option<&T>) -> Result<Option<String>> {
    value.map(|value| encode(what, value)).transpose()
}

impl RolloutRepo for Store {
    async fn create_rollout(&self, rollout: NewRollout) -> Result<Rollout> {
        let now = self.now().as_unix_ms();
        let nodes = encode("rollout nodes", &rollout.nodes)?;
        let canary = encode_opt("rollout canary", rollout.canary.as_ref())?;
        let window = encode_opt("rollout window", rollout.window.as_ref())?;
        let by = rollout.created_by.map(UserId::get);
        self.write(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "INSERT INTO rollouts (state, nodes, canary, window, created_by, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING {COLUMNS}"
                ))?
                .query_row(
                    params![
                        enum_text(RolloutState::Running),
                        nodes,
                        canary,
                        window,
                        by,
                        now
                    ],
                    rollout_from_row,
                )?)
        })
        .await
    }

    async fn update_rollout(&self, rollout: &Rollout) -> Result<()> {
        let id = rollout.id;
        let state = enum_text(rollout.state);
        let nodes = encode("rollout nodes", &rollout.nodes)?;
        let waiting = encode_opt("rollout wait", rollout.waiting.as_ref())?;
        let error = rollout.error.clone();
        let finished_at = rollout.finished_at.map(Timestamp::as_unix_ms);
        self.write(move |conn| {
            let updated = conn
                .prepare_cached(
                    "UPDATE rollouts SET state = ?2, nodes = ?3, waiting = ?4, error = ?5,
                     finished_at = ?6 WHERE id = ?1",
                )?
                .execute(params![id.get(), state, nodes, waiting, error, finished_at])?;
            if updated == 0 {
                return Err(Error::NotFound {
                    what: "rollout",
                    key: id.to_string(),
                });
            }
            Ok(())
        })
        .await
    }

    async fn rollout(&self, id: RolloutId) -> Result<Option<Rollout>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!("SELECT {COLUMNS} FROM rollouts WHERE id = ?1"))?
                .query_row([id.get()], rollout_from_row)
                .optional()?)
        })
        .await
    }

    async fn rollouts(&self, limit: usize) -> Result<Vec<Rollout>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM rollouts ORDER BY id DESC LIMIT ?1"
                ))?
                .query_map([limit], rollout_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn active_rollout(&self) -> Result<Option<Rollout>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM rollouts
                     WHERE state IN ('running', 'paused', 'halted') ORDER BY id DESC LIMIT 1"
                ))?
                .query_row([], rollout_from_row)
                .optional()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::{JobId, NodeId};
    use uniwrt_model::rollout::{
        Canary, NewRollout, RolloutNode, RolloutNodeState, RolloutState, RolloutWait,
        UpgradeChoice, WINDOW,
    };

    use crate::{RolloutRepo as _, testing};

    fn node(id: i64) -> RolloutNode {
        RolloutNode {
            node: NodeId::new(id),
            name: format!("n{id}"),
            choice: UpgradeChoice::Asu {
                release: "25.12.1".to_owned(),
            },
            from: "24.10.4".to_owned(),
            state: RolloutNodeState::Pending,
            job: None,
            detail: None,
        }
    }

    #[tokio::test]
    async fn a_rollout_is_kept_and_found_while_active() {
        let (_dir, store, _) = testing::open().await;
        assert!(store.active_rollout().await.unwrap().is_none());
        let mut rollout = store
            .create_rollout(NewRollout {
                nodes: vec![node(2), node(1)],
                canary: Some(Canary { soak_min: 60 }),
                window: Some(WINDOW),
                created_by: None,
            })
            .await
            .unwrap();
        assert_eq!(rollout.state, RolloutState::Running);
        rollout.nodes[0].state = RolloutNodeState::Upgrading;
        rollout.nodes[0].job = Some(JobId::new(7));
        rollout.waiting = Some(RolloutWait::Busy {
            why: "an apply".to_owned(),
        });
        store.update_rollout(&rollout).await.unwrap();
        assert_eq!(
            store.active_rollout().await.unwrap().as_ref(),
            Some(&rollout)
        );
        rollout.state = RolloutState::Done;
        rollout.finished_at = Some(rollout.started_at);
        store.update_rollout(&rollout).await.unwrap();
        assert!(store.active_rollout().await.unwrap().is_none());
        assert_eq!(store.rollouts(5).await.unwrap(), [rollout.clone()]);
        assert_eq!(store.rollout(rollout.id).await.unwrap(), Some(rollout));
    }
}

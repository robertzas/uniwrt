//! [`JobRepo`] on SQLite: the `jobs` table, each job's steps and error as JSON.

use rusqlite::{Connection, OptionalExtension as _, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp, UserId};
use uniwrt_model::job::{Job, JobError, JobNodeState, JobState, NewJob, Step, StepState};

use super::JobRepo;
use crate::{
    Error, Result, Store,
    codec::{enum_column, enum_text, timestamp_column},
};

const COLUMNS: &str = "id, kind, state, target, node_id, created_by, started_at, finished_at, \
     steps, error, revision_id, nodes";

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: JobId::new(row.get(0)?),
        kind: enum_column(row, 1)?,
        state: enum_column(row, 2)?,
        target: row.get(3)?,
        node: row.get::<_, Option<i64>>(4)?.map(NodeId::new),
        created_by: row.get::<_, Option<i64>>(5)?.map(UserId::new),
        started_at: timestamp_column(row, 6)?,
        finished_at: row.get::<_, Option<i64>>(7)?.map(Timestamp::from_unix_ms),
        steps: json_column(row, 8)?,
        error: row
            .get::<_, Option<String>>(9)?
            .map(|text| decode(9, &text))
            .transpose()?,
        revision: row.get::<_, Option<i64>>(10)?.map(RevisionId::new),
        nodes: json_column(row, 11)?,
    })
}

fn json_column<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    decode(index, &row.get::<_, String>(index)?)
}

fn decode<T: DeserializeOwned>(index: usize, text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn encode(what: &'static str, value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|source| Error::Encode { what, source })
}

impl JobRepo for Store {
    async fn create_job(&self, job: NewJob) -> Result<Job> {
        let now = self.now().as_unix_ms();
        let steps: Vec<Step> = job.steps.into_iter().map(Step::pending).collect();
        let steps = encode("job steps", &steps)?;
        let nodes = encode("job nodes", &job.nodes)?;
        self.write(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "INSERT INTO jobs (kind, state, target, node_id, created_by, started_at, steps,
                     revision_id, nodes)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING {COLUMNS}"
                ))?
                .query_row(
                    params![
                        enum_text(job.kind),
                        enum_text(JobState::Running),
                        job.target,
                        job.node.map(NodeId::get),
                        job.created_by.map(UserId::get),
                        now,
                        steps,
                        job.revision.map(RevisionId::get),
                        nodes,
                    ],
                    job_from_row,
                )?)
        })
        .await
    }

    async fn update_job(&self, job: &Job) -> Result<()> {
        let steps = encode("job steps", &job.steps)?;
        let error = job
            .error
            .as_ref()
            .map(|error| encode("job error", error))
            .transpose()?;
        let nodes = encode("job nodes", &job.nodes)?;
        let (id, state, node) = (job.id, enum_text(job.state), job.node.map(NodeId::get));
        let finished_at = job.finished_at.map(Timestamp::as_unix_ms);
        self.write(move |conn| {
            let updated = conn
                .prepare_cached(
                    "UPDATE jobs SET state = ?2, node_id = ?3, finished_at = ?4, steps = ?5,
                     error = ?6, nodes = ?7 WHERE id = ?1",
                )?
                .execute(params![
                    id.get(),
                    state,
                    node,
                    finished_at,
                    steps,
                    error,
                    nodes
                ])?;
            if updated == 0 {
                return Err(Error::NotFound {
                    what: "job",
                    key: id.to_string(),
                });
            }
            Ok(())
        })
        .await
    }

    async fn job(&self, id: JobId) -> Result<Option<Job>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!("SELECT {COLUMNS} FROM jobs WHERE id = ?1"))?
                .query_row([id.get()], job_from_row)
                .optional()?)
        })
        .await
    }

    async fn jobs(&self, limit: usize) -> Result<Vec<Job>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(&format!(
                    "SELECT {COLUMNS} FROM jobs ORDER BY id DESC LIMIT ?1"
                ))?
                .query_map([limit], job_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn interrupt_jobs(&self, error: JobError) -> Result<usize> {
        let now = self.now();
        self.write(move |conn| interrupt(conn, now, &error)).await
    }
}

/// Fails every running job, and its running step, with `error`.
fn interrupt(conn: &Connection, now: Timestamp, error: &JobError) -> Result<usize> {
    let running: Vec<Job> = conn
        .prepare_cached(&format!(
            "SELECT {COLUMNS} FROM jobs WHERE state = 'running'"
        ))?
        .query_map([], job_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    let error_json = encode("job error", error)?;
    for job in &running {
        let mut steps = job.steps.clone();
        for step in &mut steps {
            if step.state == StepState::Running {
                step.state = StepState::Failed;
                step.finished_at = Some(now);
            }
        }
        // An apply's unconfirmed nodes roll back by themselves (D-042, D-069).
        let mut nodes = job.nodes.clone();
        for node in &mut nodes {
            if matches!(
                node.state,
                JobNodeState::Pending
                    | JobNodeState::Staged
                    | JobNodeState::Applied
                    | JobNodeState::Verified
            ) {
                node.state = JobNodeState::RolledBack;
                node.detail = Some("Not confirmed: it rolls back by itself".to_owned());
            }
        }
        conn.prepare_cached(
            "UPDATE jobs SET state = ?2, finished_at = ?3, steps = ?4, error = ?5, nodes = ?6
             WHERE id = ?1",
        )?
        .execute(params![
            job.id.get(),
            enum_text(JobState::Failed),
            now.as_unix_ms(),
            encode("job steps", &steps)?,
            error_json,
            encode("job nodes", &nodes)?,
        ])?;
    }
    Ok(running.len())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::{Clock as _, JobId, NodeId, RevisionId, UserId};
    use uniwrt_model::job::{
        JobError, JobErrorCode, JobKind, JobNode, JobNodeState, JobState, NewJob, StepName,
        StepState,
    };

    use crate::{JobRepo as _, testing};

    fn new_job(target: &str) -> NewJob {
        NewJob {
            node: None,
            kind: JobKind::Adopt,
            target: target.to_owned(),
            revision: None,
            created_by: Some(UserId::new(1)),
            steps: vec![StepName::Connect, StepName::Check],
            nodes: Vec::new(),
        }
    }

    fn job_node(id: i64, state: JobNodeState) -> JobNode {
        JobNode {
            node: NodeId::new(id),
            name: format!("ap-{id}"),
            state,
            detail: None,
        }
    }

    #[tokio::test]
    async fn jobs_start_running_with_pending_steps_and_save_their_progress() {
        let (_dir, store, clock) = testing::open().await;
        let mut job = store.create_job(new_job("10.0.0.2:22")).await.unwrap();
        assert_eq!(job.state, JobState::Running);
        assert_eq!(job.started_at, testing::START);
        assert!(
            job.steps
                .iter()
                .all(|step| step.state == StepState::Pending)
        );
        assert_eq!(store.job(job.id).await.unwrap().as_ref(), Some(&job));

        clock.advance(Duration::from_secs(3));
        job.steps[0].state = StepState::Done;
        job.steps[1].state = StepState::Failed;
        job.state = JobState::Failed;
        job.node = Some(NodeId::new(7));
        job.nodes = vec![job_node(7, JobNodeState::Failed)];
        job.finished_at = Some(clock.now());
        job.error = Some(JobError {
            code: JobErrorCode::Unsupported,
            message: "OpenWrt 23.05 is too old".to_owned(),
        });
        store.update_job(&job).await.unwrap();
        assert_eq!(store.job(job.id).await.unwrap(), Some(job));
        assert!(store.job(JobId::new(99)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_list_is_newest_first_and_limited() {
        let (_dir, store, _clock) = testing::open().await;
        for octet in 1..=3 {
            store
                .create_job(new_job(&format!("10.0.0.{octet}:22")))
                .await
                .unwrap();
        }
        let targets: Vec<_> = store
            .jobs(2)
            .await
            .unwrap()
            .into_iter()
            .map(|job| job.target)
            .collect();
        assert_eq!(targets, ["10.0.0.3:22", "10.0.0.2:22"]);
    }

    #[tokio::test]
    async fn interrupted_jobs_fail_with_their_running_step() {
        let (_dir, store, clock) = testing::open().await;
        let mut running = store
            .create_job(NewJob {
                kind: JobKind::Apply,
                revision: Some(RevisionId::new(4)),
                nodes: vec![
                    job_node(1, JobNodeState::Applied),
                    job_node(2, JobNodeState::Unchanged),
                ],
                ..new_job("site")
            })
            .await
            .unwrap();
        assert_eq!(running.revision, Some(RevisionId::new(4)));
        running.steps[0].state = StepState::Done;
        running.steps[1].state = StepState::Running;
        store.update_job(&running).await.unwrap();
        let mut done = store.create_job(new_job("10.0.0.3:22")).await.unwrap();
        done.state = JobState::Succeeded;
        store.update_job(&done).await.unwrap();

        clock.advance(Duration::from_secs(60));
        let error = JobError {
            code: JobErrorCode::Interrupted,
            message: "The controller stopped".to_owned(),
        };
        assert_eq!(store.interrupt_jobs(error.clone()).await.unwrap(), 1);
        let job = store.job(running.id).await.unwrap().unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error, Some(error.clone()));
        assert_eq!(job.finished_at, Some(clock.now()));
        let states: Vec<_> = job.steps.iter().map(|step| step.state).collect();
        assert_eq!(states, [StepState::Done, StepState::Failed]);
        let nodes: Vec<_> = job.nodes.iter().map(|node| node.state).collect();
        assert_eq!(nodes, [JobNodeState::RolledBack, JobNodeState::Unchanged]);
        assert_eq!(store.job(done.id).await.unwrap(), Some(done));
        assert_eq!(store.interrupt_jobs(error).await.unwrap(), 0);
    }
}

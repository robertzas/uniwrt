//! [`SpeedTestRepo`] on SQLite: the `speed_tests` table.

use rusqlite::params;
use uniwrt_core::{JobId, NodeId};
use uniwrt_model::speed::{Grade, SpeedTest};

use super::SpeedTestRepo;
use crate::{Result, Store, codec::timestamp_column};

/// The most tests kept: years of weekly tests, plus manual ones.
const KEEP: i64 = 500;

fn grade_text(grade: Grade) -> &'static str {
    grade.label()
}

fn grade_column(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Grade> {
    let text: String = row.get(i)?;
    [
        Grade::APlus,
        Grade::A,
        Grade::B,
        Grade::C,
        Grade::D,
        Grade::F,
    ]
    .into_iter()
    .find(|grade| grade.label() == text)
    .ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            i,
            rusqlite::types::Type::Text,
            format!("unknown grade {text:?}").into(),
        )
    })
}

fn rate(value: u32) -> i64 {
    i64::from(value)
}

fn unsigned(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u32> {
    let value: i64 = row.get(i)?;
    u32::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Integer, err.into())
    })
}

impl SpeedTestRepo for Store {
    async fn record_speed_test(&self, test: SpeedTest) -> Result<()> {
        self.write(move |conn| {
            conn.prepare_cached(
                "INSERT INTO speed_tests (job_id, node_id, at, bypassed, download_kbps,
                     upload_kbps, idle_us, download_us, upload_us, grade)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                test.job.get(),
                test.node.get(),
                test.at.as_unix_ms(),
                test.bypassed,
                rate(test.download_kbps),
                rate(test.upload_kbps),
                rate(test.idle_us),
                rate(test.download_us),
                rate(test.upload_us),
                grade_text(test.grade),
            ])?;
            conn.prepare_cached(
                "DELETE FROM speed_tests WHERE job_id NOT IN
                     (SELECT job_id FROM speed_tests ORDER BY at DESC, job_id DESC LIMIT ?1)",
            )?
            .execute([KEEP])?;
            Ok(())
        })
        .await
    }

    async fn speed_tests(&self, limit: usize) -> Result<Vec<SpeedTest>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT job_id, node_id, at, bypassed, download_kbps, upload_kbps, idle_us,
                         download_us, upload_us, grade
                     FROM speed_tests ORDER BY at DESC, job_id DESC LIMIT ?1",
                )?
                .query_map([limit], |row| {
                    Ok(SpeedTest {
                        job: JobId::new(row.get(0)?),
                        node: NodeId::new(row.get(1)?),
                        at: timestamp_column(row, 2)?,
                        bypassed: row.get(3)?,
                        download_kbps: unsigned(row, 4)?,
                        upload_kbps: unsigned(row, 5)?,
                        idle_us: unsigned(row, 6)?,
                        download_us: unsigned(row, 7)?,
                        upload_us: unsigned(row, 8)?,
                        grade: grade_column(row, 9)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_model::{
        job::{JobKind, NewJob},
        speed::{Grade, SpeedTest},
    };

    use crate::{
        JobRepo as _, NodeRepo as _, SpeedTestRepo as _, repo::nodes::tests::new_node, testing,
    };

    #[tokio::test]
    async fn tests_come_back_newest_first() {
        let (_dir, store, _) = testing::open().await;
        let node = store.create_node(new_node(1)).await.unwrap().id;
        let mut tests = Vec::new();
        for (i, grade) in [Grade::C, Grade::APlus].into_iter().enumerate() {
            let job = store
                .create_job(NewJob {
                    kind: JobKind::SpeedTest,
                    target: "gw".to_owned(),
                    node: Some(node),
                    revision: None,
                    created_by: None,
                    steps: Vec::new(),
                    nodes: Vec::new(),
                })
                .await
                .unwrap();
            let test = SpeedTest {
                job: job.id,
                node,
                at: testing::START.saturating_add(Duration::from_secs(i as u64)),
                bypassed: i == 0,
                download_kbps: 940_000,
                upload_kbps: 35_000,
                idle_us: 11_500,
                download_us: 160_000,
                upload_us: 30_000,
                grade,
            };
            store.record_speed_test(test.clone()).await.unwrap();
            tests.push(test);
        }
        tests.reverse();
        assert_eq!(store.speed_tests(10).await.unwrap(), tests);
        assert_eq!(store.speed_tests(1).await.unwrap(), tests[..1]);
    }
}

//! [`RevisionRepo`] on SQLite: the `revisions` table, and the configuration secrets committed
//! with them into `vault`.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension as _, Row, params};
use uniwrt_core::{RevisionId, UserId};
use uniwrt_model::site::{
    SiteConfig,
    diff::{Change, diff},
    migrate::upgrade,
    revision::{NewRevision, Revision, RevisionMeta},
};

use super::RevisionRepo;
use crate::{
    Error, Result, SealedSecret, Store,
    codec::{enum_column, enum_text, timestamp_column},
    vault::ALG,
};

const META: &str =
    "id, parent, kind, restored_from, author_id, author, message, created_at, schema, changes";

impl RevisionRepo for Store {
    async fn head_revision(&self) -> Result<Option<Revision>> {
        self.read(|conn| {
            let query =
                format!("SELECT {META}, config, diff FROM revisions ORDER BY id DESC LIMIT 1");
            full(conn, &query, [])
        })
        .await
    }

    async fn revision(&self, id: RevisionId) -> Result<Option<Revision>> {
        self.read(move |conn| {
            let query = format!("SELECT {META}, config, diff FROM revisions WHERE id = ?1");
            full(conn, &query, [id.get()])
        })
        .await
    }

    async fn revisions(
        &self,
        before: Option<RevisionId>,
        limit: usize,
    ) -> Result<Vec<RevisionMeta>> {
        let before = before.map_or(i64::MAX, RevisionId::get);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.read(move |conn| {
            let query =
                format!("SELECT {META} FROM revisions WHERE id < ?1 ORDER BY id DESC LIMIT ?2");
            let mut statement = conn.prepare_cached(&query)?;
            let rows = statement.query_map([before, limit], meta)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    async fn commit_revision(
        &self,
        new: NewRevision,
        secrets: Vec<SealedSecret>,
    ) -> Result<RevisionMeta> {
        let now = self.now().as_unix_ms();
        let config = serde_json::to_string(&new.config).map_err(|source| Error::Encode {
            what: "revision",
            source,
        })?;
        self.write(move |conn| {
            let head: Option<(i64, String)> = conn
                .prepare_cached("SELECT id, config FROM revisions ORDER BY id DESC LIMIT 1")?
                .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()?;
            let head_id = head.as_ref().map(|(id, _)| RevisionId::new(*id));
            if head_id != new.base {
                return Err(Error::NotLatest {
                    base: label(new.base),
                    head: label(head_id),
                });
            }
            let parent = head.map(|(id, json)| document(id, &json)).transpose()?;
            // The document may refer to the secrets its parent or the revision it restores
            // refer to, and to the ones committed with it; nothing else.
            let mut known: BTreeSet<String> = secrets
                .iter()
                .map(|secret| secret.name.name().to_owned())
                .collect();
            if let Some(parent) = &parent {
                known.extend(refs(parent));
            }
            if let Some(restored) = new.restored_from {
                let json: String = conn
                    .prepare_cached("SELECT config FROM revisions WHERE id = ?1")?
                    .query_row([restored.get()], |row| row.get(0))
                    .optional()?
                    .ok_or_else(|| Error::NotFound {
                        what: "revision",
                        key: restored.to_string(),
                    })?;
                known.extend(refs(&document(restored.get(), &json)?));
            }
            if let Some(name) = refs(&new.config)
                .into_iter()
                .find(|name| !known.contains(name))
            {
                return Err(Error::UnknownSecret { name });
            }
            let changes = parent
                .as_ref()
                .map(|parent| diff(parent, &new.config))
                .unwrap_or_default();
            let count = u32::try_from(changes.len()).unwrap_or(u32::MAX);
            let changes = serde_json::to_string(&changes).map_err(|source| Error::Encode {
                what: "revision",
                source,
            })?;
            for secret in &secrets {
                conn.prepare_cached(
                    "INSERT INTO vault (name, alg, value, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                )?
                .execute(params![secret.name.name(), ALG, secret.value, now])?;
            }
            let query = format!(
                "INSERT INTO revisions (parent, kind, restored_from, author_id, author, message,
                     created_at, schema, config, diff, changes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) RETURNING {META}"
            );
            Ok(conn.prepare_cached(&query)?.query_row(
                params![
                    head_id.map(RevisionId::get),
                    enum_text(new.kind),
                    new.restored_from.map(RevisionId::get),
                    new.author_id.map(UserId::get),
                    new.author,
                    new.message,
                    now,
                    new.config.schema,
                    config,
                    changes,
                    count,
                ],
                meta,
            )?)
        })
        .await
    }
}

fn full<P: rusqlite::Params>(
    conn: &Connection,
    query: &str,
    params: P,
) -> Result<Option<Revision>> {
    let Some((meta, config, changes)) = conn
        .prepare_cached(query)?
        .query_row(params, |row| {
            Ok((
                meta(row)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let id = meta.id.get();
    let config = document(id, &config)?;
    let diff: Vec<Change> = serde_json::from_str(&changes).map_err(|source| Error::Decode {
        what: "revision diff",
        key: id.to_string(),
        source,
    })?;
    Ok(Some(Revision { meta, config, diff }))
}

/// A stored document, upgraded to the current schema.
fn document(id: i64, json: &str) -> Result<SiteConfig> {
    let value = serde_json::from_str(json).map_err(|source| Error::Decode {
        what: "revision",
        key: id.to_string(),
        source,
    })?;
    upgrade(value).map_err(|source| Error::Document { id, source })
}

fn refs(config: &SiteConfig) -> Vec<String> {
    config
        .secrets()
        .into_iter()
        .filter_map(|slot| slot.value.map(|secret| secret.name().to_owned()))
        .collect()
}

fn label(id: Option<RevisionId>) -> String {
    id.map_or_else(|| "none".to_owned(), |id| id.to_string())
}

fn meta(row: &Row<'_>) -> rusqlite::Result<RevisionMeta> {
    Ok(RevisionMeta {
        id: RevisionId::new(row.get(0)?),
        parent: row.get::<_, Option<i64>>(1)?.map(RevisionId::new),
        kind: enum_column(row, 2)?,
        restored_from: row.get::<_, Option<i64>>(3)?.map(RevisionId::new),
        author_id: row.get::<_, Option<i64>>(4)?.map(UserId::new),
        author: row.get(5)?,
        message: row.get(6)?,
        created_at: timestamp_column(row, 7)?,
        schema: row.get(8)?,
        changes: row.get(9)?,
    })
}

//! [`FloorRepo`] on SQLite: the `floors` and `floor_nodes` tables.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension as _, Row, params};
use uniwrt_core::{FloorId, NodeId};
use uniwrt_model::floor::{Floor, FloorFields, FloorImage, FloorScale, Placement, Point, Wall};

use super::{FloorRepo, NewFloorImage};
use crate::{
    Error, Result, Store,
    codec::{constraint, enum_column, enum_text, json_column, timestamp_column},
};

/// A floor's columns without the image itself.
const COLUMNS: &str = "id, name, level, height_m, scale, image_kind, image_width, image_height, \
                       length(image), image_version, updated_at, walls";

fn floor_from_row(row: &Row<'_>) -> rusqlite::Result<Floor> {
    let image = match row.get::<_, Option<String>>(5)? {
        Some(_) => Some(FloorImage {
            kind: enum_column(row, 5)?,
            width_px: row.get(6)?,
            height_px: row.get(7)?,
            bytes: row.get(8)?,
            version: row.get(9)?,
        }),
        None => None,
    };
    Ok(Floor {
        id: FloorId::new(row.get(0)?),
        name: row.get(1)?,
        level: row.get(2)?,
        height_m: row.get(3)?,
        scale: json_column(row, 4)?,
        image,
        nodes: Vec::new(),
        walls: json_column(row, 11)?,
        updated_at: timestamp_column(row, 10)?,
    })
}

fn scale_text(scale: Option<&FloorScale>) -> Result<Option<String>> {
    scale
        .map(serde_json::to_string)
        .transpose()
        .map_err(|source| Error::Encode {
            what: "a floor's scale",
            source,
        })
}

fn walls_text(walls: &[Wall]) -> Result<String> {
    serde_json::to_string(walls).map_err(|source| Error::Encode {
        what: "a floor's walls",
        source,
    })
}

/// The placements of `floor`, or of every floor, by floor.
fn placements(
    conn: &Connection,
    floor: Option<FloorId>,
) -> Result<BTreeMap<FloorId, Vec<Placement>>> {
    let mut by_floor: BTreeMap<FloorId, Vec<Placement>> = BTreeMap::new();
    let mut stmt = conn.prepare_cached(
        "SELECT floor_id, node_id, x, y FROM floor_nodes
         WHERE ?1 IS NULL OR floor_id = ?1 ORDER BY node_id",
    )?;
    let rows = stmt.query_map([floor.map(FloorId::get)], |row| {
        Ok((
            FloorId::new(row.get(0)?),
            Placement {
                node: NodeId::new(row.get(1)?),
                at: Point {
                    x: row.get(2)?,
                    y: row.get(3)?,
                },
            },
        ))
    })?;
    for row in rows {
        let (floor, placement) = row?;
        by_floor.entry(floor).or_default().push(placement);
    }
    Ok(by_floor)
}

fn read_floor(conn: &Connection, id: FloorId) -> Result<Option<Floor>> {
    let Some(mut floor) = conn
        .prepare_cached(&format!("SELECT {COLUMNS} FROM floors WHERE id = ?1"))?
        .query_row([id.get()], floor_from_row)
        .optional()?
    else {
        return Ok(None);
    };
    floor.nodes = placements(conn, Some(id))?.remove(&id).unwrap_or_default();
    Ok(Some(floor))
}

impl FloorRepo for Store {
    async fn floors(&self) -> Result<Vec<Floor>> {
        self.read(|conn| {
            let mut floors: Vec<Floor> = conn
                .prepare_cached(&format!("SELECT {COLUMNS} FROM floors ORDER BY level"))?
                .query_map([], floor_from_row)?
                .collect::<rusqlite::Result<_>>()?;
            let mut by_floor = placements(conn, None)?;
            for floor in &mut floors {
                floor.nodes = by_floor.remove(&floor.id).unwrap_or_default();
            }
            Ok(floors)
        })
        .await
    }

    async fn floor(&self, id: FloorId) -> Result<Option<Floor>> {
        self.read(move |conn| read_floor(conn, id)).await
    }

    async fn create_floor(&self, fields: FloorFields, max: usize) -> Result<Floor> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let count: i64 = conn
                .prepare_cached("SELECT count(*) FROM floors")?
                .query_row([], |row| row.get(0))?;
            if usize::try_from(count).unwrap_or(usize::MAX) >= max {
                return Err(Error::Conflict {
                    what: "the most floors",
                    key: max.to_string(),
                });
            }
            let id = conn
                .prepare_cached(
                    "INSERT INTO floors (name, level, height_m, scale, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
                )?
                .query_row(
                    params![
                        fields.name,
                        fields.level,
                        fields.height_m,
                        scale_text(fields.scale.as_ref())?,
                        now
                    ],
                    |row| row.get(0),
                )
                .map_err(|err| constraint(err, "a floor at level", fields.level))?;
            read_floor(conn, FloorId::new(id))?.ok_or(Error::NotFound {
                what: "floor",
                key: id.to_string(),
            })
        })
        .await
    }

    async fn update_floor(&self, id: FloorId, fields: FloorFields) -> Result<Option<Floor>> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let updated = conn
                .prepare_cached(
                    "UPDATE floors SET name = ?2, level = ?3, height_m = ?4, scale = ?5,
                     updated_at = ?6 WHERE id = ?1",
                )?
                .execute(params![
                    id.get(),
                    fields.name,
                    fields.level,
                    fields.height_m,
                    scale_text(fields.scale.as_ref())?,
                    now
                ])
                .map_err(|err| constraint(err, "a floor at level", fields.level))?;
            if updated == 0 {
                return Ok(None);
            }
            read_floor(conn, id)
        })
        .await
    }

    async fn delete_floor(&self, id: FloorId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM floors WHERE id = ?1")?
                .execute([id.get()])?
                > 0)
        })
        .await
    }

    async fn set_floor_image(&self, id: FloorId, image: NewFloorImage) -> Result<Option<Floor>> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let Some(old) = read_floor(conn, id)? else {
                return Ok(None);
            };
            let new = FloorImage {
                kind: image.kind,
                width_px: image.width_px,
                height_px: image.height_px,
                bytes: u32::try_from(image.data.len()).unwrap_or(u32::MAX),
                version: old.image.map_or(0, |image| image.version) + 1,
            };
            // The same drawing at another resolution: what was drawn on it follows.
            let scale = match (old.image, old.scale) {
                (Some(before), Some(scale)) => Some(FloorScale {
                    from: before.rescale(scale.from, &new),
                    to: before.rescale(scale.to, &new),
                    ..scale
                }),
                _ => None,
            };
            let walls: Vec<Wall> = match old.image {
                Some(before) => old
                    .walls
                    .iter()
                    .map(|wall| Wall {
                        from: before.rescale(wall.from, &new),
                        to: before.rescale(wall.to, &new),
                        ..*wall
                    })
                    .collect(),
                None => old.walls,
            };
            if let Some(before) = old.image {
                let mut stmt = conn
                    .prepare_cached("UPDATE floor_nodes SET x = ?2, y = ?3 WHERE node_id = ?1")?;
                for placement in &old.nodes {
                    let at = before.rescale(placement.at, &new);
                    stmt.execute(params![placement.node.get(), at.x, at.y])?;
                }
            }
            conn.prepare_cached(
                "UPDATE floors SET image_kind = ?2, image_width = ?3, image_height = ?4,
                 image_version = ?5, image = ?6, scale = ?7, updated_at = ?8, walls = ?9
                 WHERE id = ?1",
            )?
            .execute(params![
                id.get(),
                enum_text(new.kind),
                new.width_px,
                new.height_px,
                new.version,
                image.data,
                scale_text(scale.as_ref())?,
                now,
                walls_text(&walls)?
            ])?;
            read_floor(conn, id)
        })
        .await
    }

    async fn set_floor_walls(&self, id: FloorId, walls: Vec<Wall>) -> Result<Option<Floor>> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let updated = conn
                .prepare_cached("UPDATE floors SET walls = ?2, updated_at = ?3 WHERE id = ?1")?
                .execute(params![id.get(), walls_text(&walls)?, now])?;
            if updated == 0 {
                return Ok(None);
            }
            read_floor(conn, id)
        })
        .await
    }

    async fn floor_image(&self, id: FloorId) -> Result<Option<(FloorImage, Vec<u8>)>> {
        self.read(move |conn| {
            Ok(conn
                .prepare_cached(
                    "SELECT image_kind, image_width, image_height, length(image), image_version,
                     image FROM floors WHERE id = ?1 AND image IS NOT NULL",
                )?
                .query_row([id.get()], |row| {
                    Ok((
                        FloorImage {
                            kind: enum_column(row, 0)?,
                            width_px: row.get(1)?,
                            height_px: row.get(2)?,
                            bytes: row.get(3)?,
                            version: row.get(4)?,
                        },
                        row.get(5)?,
                    ))
                })
                .optional()?)
        })
        .await
    }

    async fn place_node(&self, floor: FloorId, placement: Placement) -> Result<Option<Floor>> {
        let now = self.now().as_unix_ms();
        self.write(move |conn| {
            let touched = conn
                .prepare_cached("UPDATE floors SET updated_at = ?2 WHERE id = ?1")?
                .execute(params![floor.get(), now])?;
            if touched == 0 {
                return Ok(None);
            }
            conn.prepare_cached(
                "INSERT INTO floor_nodes (node_id, floor_id, x, y) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (node_id) DO UPDATE SET
                 floor_id = excluded.floor_id, x = excluded.x, y = excluded.y",
            )?
            .execute(params![
                placement.node.get(),
                floor.get(),
                placement.at.x,
                placement.at.y
            ])
            .map_err(|err| constraint(err, "node", placement.node))?;
            read_floor(conn, floor)
        })
        .await
    }

    async fn unplace_node(&self, node: NodeId) -> Result<bool> {
        self.write(move |conn| {
            Ok(conn
                .prepare_cached("DELETE FROM floor_nodes WHERE node_id = ?1")?
                .execute([node.get()])?
                > 0)
        })
        .await
    }
}

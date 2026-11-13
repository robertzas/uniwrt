//! Floor plans (features/wifi.md, Coverage & floor plans; D-117). Everyone reads; admins edit,
//! and edits save at once (nothing here goes to a node):
//!
//! - `GET v1/floors`: every [`Floor`] by level, with its placements and its image's metadata.
//! - `POST v1/floors` ([`FloorFields`]; 201), `PUT v1/floors/{id}` (the same fields) and
//!   `DELETE v1/floors/{id}`: a floor's name, level (unique: 409), height and scale; at most
//!   [`MAX_FLOORS`] (409). Audited.
//! - `PUT v1/floors/{id}/image`: the plan as the body, a PNG or a still WebP of up to
//!   [`MAX_IMAGE_BYTES`] whose sides are [`MIN_IMAGE_SIDE`] to [`MAX_IMAGE_SIDE`] px, read from
//!   its header; the scale's line and the placements follow a new size. Audited.
//!   `GET v1/floors/{id}/image?v=<version>`: the plan, cached for good (each version has its URL).
//! - `PUT v1/floors/{id}/nodes/{node}` (`{x, y}` on the plan, which must be uploaded: 409) places
//!   a node, moving it from any other floor; `DELETE` there takes it off. Placements aren't
//!   audited (dragging a node would flood the log), like the topology's layout.

mod image;
#[cfg(test)]
mod tests;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, put},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::{FloorId, NodeId};
use uniwrt_model::{
    floor::{
        Floor, FloorFields, MAX_FLOORS, MAX_IMAGE_BYTES, MAX_IMAGE_SIDE, MIN_IMAGE_SIDE, Placement,
        Point,
    },
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, Error, FloorRepo as _, NewFloorImage};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

pub(crate) fn routes() -> Router<Api> {
    Router::new()
        .route("/v1/floors", get(list).post(create))
        .route("/v1/floors/{id}", put(update).delete(delete))
        .route(
            "/v1/floors/{id}/image",
            get(image)
                .put(upload)
                .layer(DefaultBodyLimit::max(MAX_IMAGE_BYTES)),
        )
        .route("/v1/floors/{id}/nodes/{node}", put(place).delete(unplace))
}

/// Where to place a node, in the plan's pixels.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub(crate) struct PlaceRequest {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

fn admin(user: &CurrentUser) -> ApiResult<()> {
    if user.user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Only admins edit floor plans".to_owned(),
        ))
    }
}

async fn audit(api: &Api, user: &CurrentUser, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::FloorPlan,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

fn not_found() -> ApiError {
    ApiError::NotFound("No such floor".to_owned())
}

/// The store's conflicts as people read them.
fn conflict(err: Error, level: i16) -> ApiError {
    match err {
        Error::Conflict {
            what: "the most floors",
            ..
        } => ApiError::Conflict(format!("A site has at most {MAX_FLOORS} floors")),
        Error::Conflict { .. } => {
            ApiError::Conflict(format!("Another floor is at level {level} already"))
        }
        err => err.into(),
    }
}

async fn floor(api: &Api, id: FloorId) -> ApiResult<Floor> {
    api.store().floor(id).await?.ok_or_else(not_found)
}

async fn list(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<Vec<Floor>>> {
    Ok(Json(api.store().floors().await?))
}

async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<FloorFields>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<Floor>)> {
    admin(&user)?;
    let Json(fields) = body?;
    let fields = fields
        .validate(None)
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;
    let level = fields.level;
    let floor = api
        .store()
        .create_floor(fields, MAX_FLOORS)
        .await
        .map_err(|err| conflict(err, level))?;
    let detail = format!("added floor {} (level {level})", floor.name);
    audit(&api, &user, &client, detail).await?;
    Ok((StatusCode::CREATED, Json(floor)))
}

async fn update(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
    body: Result<Json<FloorFields>, JsonRejection>,
) -> ApiResult<Json<Floor>> {
    admin(&user)?;
    let Json(fields) = body?;
    let id = FloorId::new(id);
    let before = floor(&api, id).await?;
    let fields = fields
        .validate(before.image.as_ref())
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;
    let level = fields.level;
    let after = api
        .store()
        .update_floor(id, fields)
        .await
        .map_err(|err| conflict(err, level))?
        .ok_or_else(not_found)?;
    let detail = format!("changed floor {} (level {level})", after.name);
    audit(&api, &user, &client, detail).await?;
    Ok(Json(after))
}

async fn delete(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    admin(&user)?;
    let id = FloorId::new(id);
    let gone = floor(&api, id).await?;
    if !api.store().delete_floor(id).await? {
        return Err(not_found());
    }
    let detail = format!("deleted floor {} (level {})", gone.name, gone.level);
    audit(&api, &user, &client, detail).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn image(
    State(api): State<Api>,
    _: CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    let (image, data) = api
        .store()
        .floor_image(FloorId::new(id))
        .await?
        .ok_or_else(|| ApiError::NotFound("That floor has no plan".to_owned()))?;
    Ok((
        [
            (header::CONTENT_TYPE, image.kind.mime()),
            // The URL names the version, so a version never changes.
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        data,
    ))
}

async fn upload(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
    body: Bytes,
) -> ApiResult<Json<Floor>> {
    admin(&user)?;
    let (kind, width_px, height_px) = image::sniff(&body).ok_or_else(|| {
        ApiError::BadRequest("A floor plan is uploaded as a PNG or a still WebP".to_owned())
    })?;
    let sides = MIN_IMAGE_SIDE..=MAX_IMAGE_SIDE;
    if !(sides.contains(&width_px) && sides.contains(&height_px)) {
        return Err(ApiError::BadRequest(format!(
            "A floor plan's sides are {MIN_IMAGE_SIDE} to {MAX_IMAGE_SIDE} px; this one is \
             {width_px} × {height_px}"
        )));
    }
    let image = NewFloorImage {
        kind,
        width_px,
        height_px,
        data: body.to_vec(),
    };
    let floor = api
        .store()
        .set_floor_image(FloorId::new(id), image)
        .await?
        .ok_or_else(not_found)?;
    let detail = format!(
        "uploaded the plan of floor {} ({width_px} × {height_px} px)",
        floor.name
    );
    audit(&api, &user, &client, detail).await?;
    Ok(Json(floor))
}

async fn place(
    State(api): State<Api>,
    user: CurrentUser,
    Path((id, node)): Path<(i64, i64)>,
    body: Result<Json<PlaceRequest>, JsonRejection>,
) -> ApiResult<Json<Floor>> {
    admin(&user)?;
    let Json(PlaceRequest { x, y }) = body?;
    let id = FloorId::new(id);
    let plan = floor(&api, id).await?.image.ok_or_else(|| {
        ApiError::Conflict("Upload the floor's plan before placing nodes on it".to_owned())
    })?;
    let at = Point { x, y };
    if !plan.contains(at) {
        return Err(ApiError::BadRequest(format!(
            "A node is placed on the plan: x from 0 to {}, y from 0 to {}",
            plan.width_px, plan.height_px
        )));
    }
    let placement = Placement {
        node: NodeId::new(node),
        at,
    };
    match api.store().place_node(id, placement).await {
        Ok(floor) => Ok(Json(floor.ok_or_else(not_found)?)),
        Err(Error::NotFound { .. }) => Err(ApiError::NotFound("No such node".to_owned())),
        Err(err) => Err(err.into()),
    }
}

async fn unplace(
    State(api): State<Api>,
    user: CurrentUser,
    Path((id, node)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    admin(&user)?;
    let node = NodeId::new(node);
    let on = floor(&api, FloorId::new(id)).await?;
    if !on.nodes.iter().any(|placement| placement.node == node)
        || !api.store().unplace_node(node).await?
    {
        return Err(ApiError::NotFound(
            "That node isn't placed on this floor".to_owned(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

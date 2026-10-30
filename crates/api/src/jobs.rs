//! The job routes, with a session: `GET /api/v1/jobs` (the latest, newest first; `?limit=`, 50
//! by default) and `GET /api/v1/jobs/{id}`. The WS hub's `jobs` topic carries their changes.

use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
};
use serde::Deserialize;
use uniwrt_core::JobId;
use uniwrt_model::job::Job;

use crate::{Api, ApiError, ApiResult, CurrentUser, problem::error_chain};

/// How many jobs a list has unless asked for another number, and at most.
pub(crate) const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> ApiResult<Json<Vec<Job>>> {
    let Query(query) = query
        .map_err(|_| ApiError::BadRequest("The only parameter is limit, a number".to_owned()))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "The limit must be 1 to {MAX_LIMIT}"
        )));
    }
    Ok(Json(api.jobs().jobs(limit).await.map_err(jobs_error)?))
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<Job>> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A job ID is a number".to_owned()))?;
    let id = JobId::new(id);
    api.jobs()
        .job(id)
        .await
        .map_err(jobs_error)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("There is no job {id}")))
}

/// What the job runner's errors mean to a client.
pub(crate) fn jobs_error(err: uniwrt_jobs::Error) -> ApiError {
    use uniwrt_jobs::Error;
    match err {
        Error::Unavailable => ApiError::Conflict(
            "Demo mode simulates its nodes; real ones cannot be adopted".to_owned(),
        ),
        Error::Busy { target } => {
            ApiError::Conflict(format!("A job is running for {target} already"))
        }
        Error::Node(err) => ApiError::BadGateway(error_chain(&err)),
        Error::Store(err) => err.into(),
        _ => ApiError::Internal(err.to_string()),
    }
}

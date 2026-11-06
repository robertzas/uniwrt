//! Speed tests from the gateway (features/wan.md, SQM; D-076):
//!
//! - `GET /api/v1/wan/speed-tests?limit=<n>` (default 50, at most 500): the history, newest
//!   first, with what the newest test of the line suggests for SQM against the latest revision
//!   ([`SpeedTests`]).
//! - `POST /api/v1/wan/speed-test` (admins; audited) with a [`SpeedTestBody`]: measures the
//!   line from the gateway, answering 202 with the job (`speed_test`), which
//!   `GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow. Stopping SQM for it needs
//!   a managed gateway (409).

use axum::{
    Json,
    extract::{
        Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_model::{speed::SpeedTests, user::AuditAction};
use uniwrt_store::{RevisionRepo as _, SpeedTestRepo as _};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    jobs::jobs_error,
};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

/// A speed test to run.
#[derive(Debug, Default, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SpeedTestBody {
    /// Stop SQM while loading, to measure the line's own rates (autotune).
    #[serde(default)]
    #[ts(optional)]
    pub bypass_sqm: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _user: CurrentUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> ApiResult<Json<SpeedTests>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let tests = api.store().speed_tests(limit).await?;
    let head = api.store().head_revision().await?;
    let sqm = head.as_ref().and_then(|head| head.config.wan.sqm.as_ref());
    Ok(Json(SpeedTests::new(tests, sqm)))
}

pub(crate) async fn start(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<SpeedTestBody>, JsonRejection>,
) -> ApiResult<Response> {
    admin(&user, "run a speed test")?;
    let Json(body) = body?;
    let request = uniwrt_jobs::SpeedTestRequest {
        bypass_sqm: body.bypass_sqm.unwrap_or(false),
    };
    let job = api
        .jobs()
        .speed_test(request, Some(user.user.id))
        .await
        .map_err(start_error)?;
    audit(
        &api,
        &user,
        &client,
        AuditAction::SpeedTest,
        format!("job {}", job.id),
    )
    .await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

fn start_error(err: uniwrt_jobs::Error) -> ApiError {
    use uniwrt_jobs::Error;
    match err {
        Error::NoGateway => ApiError::Conflict("No gateway is adopted".to_owned()),
        Error::Observed(_) => ApiError::Conflict(
            "The gateway is in observe mode: turn its management on to stop SQM for a test"
                .to_owned(),
        ),
        err => jobs_error(err),
    }
}

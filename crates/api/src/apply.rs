//! Applying the site's configuration, with an admin's session (features/lifecycle.md, Render →
//! diff → apply; D-069):
//!
//! - `GET /api/v1/apply/plan`: what applying the latest revision would do to each node
//!   ([`ApplyPlan`]), read fresh from the nodes, secrets masked; nothing changes on them.
//! - `POST /api/v1/apply` with `{"revision":<id>}` ([`ApplyBody`]), the revision the admin
//!   reviewed, which must still be the latest (409 otherwise): starts the apply job and answers
//!   202 with it, which `GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow. Audited.
//! - `GET /api/v1/apply/applied`: the revision each node holds ([`AppliedRevision`]s).

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::RevisionId;
use uniwrt_model::{
    apply::{AppliedRevision, ApplyPlan},
    user::AuditAction,
};
use uniwrt_store::ApplyRepo as _;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    jobs::jobs_error,
    site::ensure_first_revision,
};

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ApplyBody {
    /// The revision reviewed: the latest one.
    #[ts(type = "number")]
    pub revision: i64,
}

pub(crate) async fn plan(State(api): State<Api>, user: CurrentUser) -> ApiResult<Json<ApplyPlan>> {
    admin(&user, "plan applies")?;
    ensure_first_revision(&api, None).await?;
    Ok(Json(api.jobs().plan().await.map_err(apply_error)?))
}

pub(crate) async fn start(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ApplyBody>, JsonRejection>,
) -> ApiResult<Response> {
    admin(&user, "apply the configuration")?;
    let Json(ApplyBody { revision }) = body?;
    let job = api
        .jobs()
        .apply(RevisionId::new(revision), Some(user.user.id))
        .await
        .map_err(apply_error)?;
    let detail = format!("applied revision {revision}, job {}", job.id);
    audit(&api, &user, &client, AuditAction::Apply, detail).await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

pub(crate) async fn applied(
    State(api): State<Api>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<AppliedRevision>>> {
    Ok(Json(api.store().applied_revisions().await?))
}

fn apply_error(err: uniwrt_jobs::Error) -> ApiError {
    use uniwrt_jobs::Error;
    match err {
        Error::NoRevision => ApiError::Conflict("The site has no revision yet".to_owned()),
        Error::Stale { revision, head } => ApiError::Conflict(format!(
            "Revision {revision} is no longer the latest: review revision {head}"
        )),
        Error::Failed(message) => ApiError::BadGateway(message),
        err => jobs_error(err),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use uniwrt_model::user::AuditAction;
    use uniwrt_store::{AuditRepo as _, RevisionRepo as _};

    use crate::testing::{Demo, json, request};

    #[tokio::test]
    async fn the_plan_skips_observed_nodes_and_an_apply_of_the_latest_starts() {
        let demo = Demo::start().await;
        let res = demo
            .send(request(
                Method::GET,
                "/v1/apply/plan",
                None,
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        let plan = json(res).await;
        let head = demo.store.head_revision().await.unwrap().unwrap().meta.id;
        assert_eq!(plan["revision"], head.get());
        let actions: Vec<_> = plan["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["action"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(actions.len(), demo.ids.len());
        assert!(actions.iter().all(|action| action == "skipped"));

        let stale = json!({"revision": head.get() + 1});
        let res = demo
            .send(request(
                Method::POST,
                "/v1/apply",
                Some(&stale),
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = json!({"revision": head.get()});
        let res = demo
            .send(request(
                Method::POST,
                "/v1/apply",
                Some(&body),
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        let job = json(res).await;
        assert_eq!(
            (job["kind"].as_str(), &job["revision"]),
            (Some("apply"), &json!(head.get()))
        );
        assert!(
            job["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|n| n["state"] == "skipped")
        );
        let audit = demo.store.audit_log(10).await.unwrap();
        assert!(audit.iter().any(|entry| entry.action == AuditAction::Apply));

        let res = demo
            .send(request(
                Method::GET,
                "/v1/apply/applied",
                None,
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(json(res).await.is_array());
    }

    #[tokio::test]
    async fn bad_bodies_sessions_and_controllers_without_a_dial_are_refused() {
        let demo = Demo::start_with(false).await;
        let res = demo
            .send(request(
                Method::GET,
                "/v1/apply/plan",
                None,
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let res = demo
            .send(request(
                Method::POST,
                "/v1/apply",
                Some(&json!({"revision": "latest"})),
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = demo
            .send(request(
                Method::POST,
                "/v1/apply",
                Some(&json!({"revision": 1})),
                None,
            ))
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

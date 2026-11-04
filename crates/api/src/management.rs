//! Management on and off, with an admin's session (directive 8; D-069):
//! `PUT /api/v1/nodes/{id}/management` with `{"enabled":true}` starts the job that installs the
//! transaction tool and the boot guard on the node and records it as managed; `false` removes
//! them and puts the node back in observe mode. It answers 202 with the job, which
//! `GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow.

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ManagementBody {
    /// Managed (`true`) or observe mode.
    pub enabled: bool,
}

pub(crate) async fn put(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<ManagementBody>, JsonRejection>,
) -> ApiResult<Response> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let Json(ManagementBody { enabled }) = body?;
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins switch the management of nodes".to_owned(),
        ));
    }
    let job = api
        .jobs()
        .manage(NodeId::new(id), enabled, Some(user.id))
        .await
        .map_err(jobs_error)?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Manage,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "{} management of {} (node {id}), job {}",
                if enabled { "enabled" } else { "disabled" },
                job.target,
                job.id
            )),
        })
        .await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use uniwrt_model::user::AuditAction;
    use uniwrt_store::AuditRepo as _;

    use crate::testing::{Demo, json, request};

    #[tokio::test]
    async fn an_admin_starts_the_job_and_it_is_audited() {
        let demo = Demo::start().await;
        let uri = format!("/v1/nodes/{}/management", demo.ids[1]);
        let res = demo
            .send(request(
                Method::PUT,
                &uri,
                Some(&json!({"enabled": true})),
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        let location = res.headers()["location"].to_str().unwrap().to_owned();
        let job = json(res).await;
        assert_eq!(location, format!("/api/v1/jobs/{}", job["id"]));
        assert_eq!(job["kind"], "manage");
        assert_eq!(job["node"], demo.ids[1].get());
        let steps: Vec<_> = job["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| step["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(steps, ["connect", "install_tool", "record"]);
        let audit = demo.store.audit_log(10).await.unwrap();
        assert!(audit.iter().any(|entry| {
            entry.action == AuditAction::Manage
                && entry
                    .detail
                    .as_deref()
                    .unwrap()
                    .starts_with("enabled management of")
        }));
    }

    #[tokio::test]
    async fn bad_requests_and_unknown_nodes_are_refused() {
        let demo = Demo::start().await;
        let send = |uri: String, body: serde_json::Value| {
            demo.send(request(Method::PUT, &uri, Some(&body), Some(&demo.cookie)))
        };
        let node = format!("/v1/nodes/{}/management", demo.ids[0]);
        let res = send(node.clone(), json!({"enabled": "yes"})).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send(node, json!({"enabled": true, "extra": 1})).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send(
            "/v1/nodes/x/management".to_owned(),
            json!({"enabled": true}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send(
            "/v1/nodes/999/management".to_owned(),
            json!({"enabled": true}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        // Without a session.
        let uri = format!("/v1/nodes/{}/management", demo.ids[0]);
        let res = demo
            .send(request(
                Method::PUT,
                &uri,
                Some(&json!({"enabled": true})),
                None,
            ))
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn without_a_dial_nothing_is_managed() {
        let demo = Demo::start_with(false).await;
        let uri = format!("/v1/nodes/{}/management", demo.ids[0]);
        let res = demo
            .send(request(
                Method::PUT,
                &uri,
                Some(&json!({"enabled": true})),
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }
}

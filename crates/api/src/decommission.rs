//! A node leaving the site, with an admin's session (features/lifecycle.md, Replace / RMA /
//! decommission; D-131), each a job answered 202 with it (`GET /api/v1/jobs/{id}` and the WS
//! hub's `jobs` topic follow it):
//!
//! - `POST v1/nodes/{id}/decommission` with `{"firstboot": bool}`: the node leaves the mesh, its
//!   uniwrt footprint and the controller's key are removed (and `firstboot` resets it), then the
//!   controller forgets it.
//! - `DELETE v1/nodes/{id}`: the controller forgets it alone, for a node it can't reach.
//!
//! Forgetting stops watching the node, saves a revision without its settings (kind `remove`)
//! when the latest has any, and deletes its record and history. Refused (409) for the gateway
//! while other nodes are adopted, for a node an active rollout still has to upgrade, and while
//! an apply or another job for the node runs. Audited (`decommission`).

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
use uniwrt_jobs::Author;
use uniwrt_model::{
    job::Job,
    user::{AuditAction, AuditEntry, Role, User},
};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DecommissionBody {
    /// Reset the device to its image's defaults (`firstboot`) and restart it, after the wipe.
    pub firstboot: bool,
}

pub(crate) async fn post(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<DecommissionBody>, JsonRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let Json(DecommissionBody { firstboot }) = body?;
    let user = admin(user)?;
    let job = api
        .jobs()
        .decommission(id, firstboot, Some(author(&user)))
        .await
        .map_err(jobs_error)?;
    let how = if firstboot {
        "decommissioned with firstboot"
    } else {
        "decommissioned"
    };
    accepted(&api, &user, &client, job, how).await
}

pub(crate) async fn delete(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let user = admin(user)?;
    let job = api
        .jobs()
        .forget(id, Some(author(&user)))
        .await
        .map_err(jobs_error)?;
    accepted(&api, &user, &client, job, "forgot").await
}

fn node_id(id: Result<Path<i64>, PathRejection>) -> ApiResult<NodeId> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    Ok(NodeId::new(id))
}

fn admin(user: CurrentUser) -> ApiResult<User> {
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins remove devices from the site".to_owned(),
        ));
    }
    Ok(user)
}

fn author(user: &User) -> Author {
    Author {
        id: user.id,
        name: user.username.clone(),
    }
}

async fn accepted(
    api: &Api,
    user: &User,
    client: &Client,
    job: Job,
    how: &str,
) -> ApiResult<Response> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Decommission,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "{how} {} (node {}), job {}",
                job.target,
                job.node.map_or_default(|node| node.to_string()),
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
    async fn an_admin_decommissions_or_forgets_an_access_point_and_it_is_audited() {
        let demo = Demo::start().await;
        let ap = demo.ids[1];
        let uri = format!("/v1/nodes/{ap}/decommission");
        let body = json!({"firstboot": true});
        let res = demo
            .send(request(Method::POST, &uri, Some(&body), Some(&demo.cookie)))
            .await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        let job = json(res).await;
        assert_eq!(
            (job["kind"].as_str(), job["node"].as_i64()),
            (Some("decommission"), Some(ap.get()))
        );
        let steps: Vec<_> = job["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| step["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(steps, ["connect", "remove_tool", "wipe", "record"]);

        let other = demo.ids[2];
        let res = demo
            .send(request(
                Method::DELETE,
                &format!("/v1/nodes/{other}"),
                None,
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        assert_eq!(json(res).await["kind"], "forget");

        let audit = demo.store.audit_log(10).await.unwrap();
        let details: Vec<_> = audit
            .iter()
            .filter(|entry| entry.action == AuditAction::Decommission)
            .filter_map(|entry| entry.detail.clone())
            .collect();
        assert!(
            details
                .iter()
                .any(|d| d.starts_with("decommissioned with firstboot")),
            "{details:?}"
        );
        assert!(
            details.iter().any(|d| d.starts_with("forgot")),
            "{details:?}"
        );
    }

    #[tokio::test]
    async fn the_gateway_bad_requests_and_unknown_nodes_are_refused() {
        let demo = Demo::start().await;
        let send = |method: Method, uri: String, body: Option<serde_json::Value>| {
            demo.send(request(method, &uri, body.as_ref(), Some(&demo.cookie)))
        };
        let gateway = demo.ids[0];
        let res = send(Method::DELETE, format!("/v1/nodes/{gateway}"), None).await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let uri = format!("/v1/nodes/{gateway}/decommission");
        let res = send(Method::POST, uri.clone(), Some(json!({"firstboot": false}))).await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let res = send(Method::POST, uri, Some(json!({}))).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send(Method::DELETE, "/v1/nodes/999".to_owned(), None).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = send(Method::DELETE, "/v1/nodes/x".to_owned(), None).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = demo
            .send(request(
                Method::DELETE,
                &format!("/v1/nodes/{}", demo.ids[1]),
                None,
                None,
            ))
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

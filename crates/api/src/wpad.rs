//! The wpad swap, with an admin's session (D-085, D-089): `POST /api/v1/nodes/{id}/wpad` starts
//! the job that swaps a managed node's basic wpad for the full one, which adds 802.11v. It
//! answers 202 with the job, which `GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow;
//! 409 for a node in observe mode.

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use uniwrt_core::NodeId;
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

pub(crate) async fn post(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Response> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins install packages on devices".to_owned(),
        ));
    }
    let job = api
        .jobs()
        .wpad(NodeId::new(id), Some(user.id))
        .await
        .map_err(|err| match err {
            uniwrt_jobs::Error::Observed(_) => ApiError::Conflict(
                "The device is in observe mode: turn its management on first".to_owned(),
            ),
            other => jobs_error(other),
        })?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Wpad,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "swapped the wpad of {} (node {id}) for the full one, job {}",
                job.target, job.id
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
    use serde_json::{Value, json as body};
    use uniwrt_model::{node::NodeMode, user::AuditAction};
    use uniwrt_store::{AuditRepo as _, NodeRepo as _};

    use crate::testing::{Demo, json, request};

    async fn send(
        demo: &Demo,
        method: Method,
        uri: &str,
        body: Option<&Value>,
    ) -> (StatusCode, Value) {
        let res = demo
            .send(request(method, uri, body, Some(&demo.cookie)))
            .await;
        let status = res.status();
        (status, json(res).await)
    }

    async fn basic_wpad(demo: &Demo, id: i64) -> bool {
        let (_, nodes) = send(demo, Method::GET, "/v1/nodes", None).await;
        let node = nodes
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap();
        node["basic_wpad"].as_bool().unwrap()
    }

    #[tokio::test]
    async fn an_admin_swaps_a_managed_nodes_wpad_and_it_is_audited() {
        let demo = Demo::start().await;
        let id = demo.ids[1].get();
        let uri = format!("/v1/nodes/{id}/wpad");
        let (status, _) = send(&demo, Method::POST, &uri, None).await;
        assert_eq!(status, StatusCode::CONFLICT, "observe mode");
        // The demo's nodes run the mesh wpad, which has 802.11v.
        assert!(!basic_wpad(&demo, id).await);

        demo.store
            .set_node_mode(demo.ids[1], NodeMode::Managed)
            .await
            .unwrap();
        let (status, job) = send(&demo, Method::POST, &uri, None).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!((&job["kind"], &job["node"]), (&body!("wpad"), &body!(id)));
        let steps: Vec<_> = job["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| step["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(steps, ["connect", "check", "fetch", "swap", "verify"]);
        let audit = demo.store.audit_log(1).await.unwrap();
        assert_eq!(audit[0].action, AuditAction::Wpad);
    }

    #[tokio::test]
    async fn unknown_nodes_and_strangers_are_refused() {
        let demo = Demo::start().await;
        let (status, _) = send(&demo, Method::POST, "/v1/nodes/999/wpad", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(&demo, Method::POST, "/v1/nodes/x/wpad", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let uri = format!("/v1/nodes/{}/wpad", demo.ids[0]);
        let res = demo.send(request(Method::POST, &uri, None, None)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

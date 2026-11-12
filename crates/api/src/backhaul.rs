//! The cables that failed the wired backhaul's probe (features/mesh.md, Wired backhaul; D-097):
//! `GET /api/v1/backhaul/failures` lists them ([`BackhaulFailure`]); an admin's
//! `DELETE /api/v1/backhaul/failures/{node}/{port}/{peer}/{peer_port}` forgets one, in either
//! order of its ends, so the next apply probes it again (audited; 404 when the cable has none).

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
};
use uniwrt_core::NodeId;
use uniwrt_model::{
    apply::{BackhaulFailure, CableEnd},
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, BackhaulRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// Longer than any port name (Linux caps them at 15 bytes).
const MAX_PORT: usize = 32;

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<Vec<BackhaulFailure>>> {
    Ok(Json(api.store().backhaul_failures().await?))
}

pub(crate) async fn forget(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    path: Result<Path<(i64, String, i64, String)>, PathRejection>,
) -> ApiResult<StatusCode> {
    let Path((node, port, peer, peer_port)) = path
        .map_err(|_| ApiError::BadRequest("A cable is two node IDs and their ports".to_owned()))?;
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins forget a cable's failed probe".to_owned(),
        ));
    }
    let ends = [end(node, port)?, end(peer, peer_port)?];
    let detail = format!(
        "forgot the failed backhaul probe of the cable between node {} ({}) and node {} ({})",
        ends[0].node, ends[0].port, ends[1].node, ends[1].port
    );
    if !api.store().forget_backhaul_failure(ends).await? {
        return Err(ApiError::NotFound(
            "That cable has no failed probe".to_owned(),
        ));
    }
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Backhaul,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn end(node: i64, port: String) -> ApiResult<CableEnd> {
    if node <= 0
        || port.is_empty()
        || port.len() > MAX_PORT
        || !port.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ApiError::BadRequest(
            "A cable's end is a node ID and a port name".to_owned(),
        ));
    }
    Ok(CableEnd {
        node: NodeId::new(node),
        port,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use uniwrt_model::{
        apply::{BackhaulFailure, CableEnd},
        user::AuditAction,
    };
    use uniwrt_store::{AuditRepo as _, BackhaulRepo as _};

    use crate::testing::{Demo, json, request};

    #[tokio::test]
    async fn failed_cables_are_listed_and_forgotten() {
        let demo = Demo::start().await;
        let (a, b) = (demo.ids[1], demo.ids[2]);
        let end = |node, port: &str| CableEnd {
            node,
            port: port.to_owned(),
        };
        demo.store
            .record_backhaul_failure(BackhaulFailure {
                ends: [end(a, "eth1"), end(b, "eth0")],
                job: uniwrt_core::JobId::new(7),
                at: uniwrt_core::Timestamp::from_unix_ms(1_000),
                why: "no answer at 1532 bytes".to_owned(),
            })
            .await
            .unwrap();

        let list = json(demo.send(demo.get("/v1/backhaul/failures")).await).await;
        assert_eq!(
            list,
            json!([{
                "ends": [{ "node": a, "port": "eth1" }, { "node": b, "port": "eth0" }],
                "job": 7,
                "at": 1_000,
                "why": "no answer at 1532 bytes",
            }])
        );

        let delete = |uri: String| request(Method::DELETE, &uri, None, Some(&demo.cookie));
        // Either order names the cable.
        let res = demo
            .send(delete(format!("/v1/backhaul/failures/{b}/eth0/{a}/eth1")))
            .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let list = json(demo.send(demo.get("/v1/backhaul/failures")).await).await;
        assert_eq!(list, json!([]));
        let audit = demo.store.audit_log(10).await.unwrap();
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == AuditAction::Backhaul)
        );

        let res = demo
            .send(delete(format!("/v1/backhaul/failures/{b}/eth0/{a}/eth1")))
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = demo
            .send(delete(format!("/v1/backhaul/failures/x/eth0/{a}/eth1")))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let long = "p".repeat(33);
        let res = demo
            .send(delete(format!("/v1/backhaul/failures/{b}/{long}/{a}/eth1")))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}

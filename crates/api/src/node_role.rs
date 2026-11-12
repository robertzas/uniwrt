//! A node's role, with an admin's session (D-100): `PUT /api/v1/nodes/{id}/role` with
//! `{"role":"ap"}` stores it and tells the engine, which rebuilds the topology; the next apply
//! renders it (a router that becomes an access point loses its DHCP server, firewall and WAN,
//! whose port joins the bridge). 409 for a second gateway, for a managed gateway, and while an
//! apply or a job for the node runs. It answers the node's new summary.

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_engine::Identity;
use uniwrt_model::{
    node::NodeRole,
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, NodeRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RoleBody {
    pub role: NodeRole,
}

pub(crate) async fn put(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<RoleBody>, JsonRejection>,
) -> ApiResult<Json<RoleBody>> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let Json(RoleBody { role }) = body?;
    let user = user.user;
    if user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins change the role of nodes".to_owned(),
        ));
    }
    let id = NodeId::new(id);
    let before = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    let node = api.jobs().set_role(id, role).await.map_err(jobs_error)?;
    if before.role == node.role {
        return Ok(Json(RoleBody { role }));
    }
    api.fleet().identify(id, Identity::from(&node));
    api.node_changed(id);
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::NodeRole,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "{} (node {id}): {} to {}",
                node.name,
                role_name(before.role),
                role_name(node.role)
            )),
        })
        .await?;
    Ok(Json(RoleBody { role: node.role }))
}

fn role_name(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Gateway => "gateway",
        NodeRole::Ap => "access point",
        NodeRole::Bridge => "bridge",
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use uniwrt_model::{node::NodeRole, user::AuditAction};
    use uniwrt_store::{AuditRepo as _, NodeRepo as _};

    use crate::testing::{Demo, json, request};

    #[tokio::test]
    async fn an_admin_changes_a_role_the_engine_and_audit_follow() {
        let demo = Demo::start().await;
        let send = |id: uniwrt_core::NodeId, body: serde_json::Value| {
            let uri = format!("/v1/nodes/{id}/role");
            demo.send(request(Method::PUT, &uri, Some(&body), Some(&demo.cookie)))
        };
        let (gateway, ap) = (demo.ids[0], demo.ids[1]);
        // A second gateway is refused.
        let res = send(ap, json!({"role": "gateway"})).await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        // The gateway in observe mode may become an access point, then the AP the gateway.
        let res = send(gateway, json!({"role": "ap"})).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json(res).await, json!({"role": "ap"}));
        let res = send(ap, json!({"role": "gateway"})).await;
        assert_eq!(res.status(), StatusCode::OK);
        let stored = demo.store.node(ap).await.unwrap().unwrap();
        assert_eq!(stored.role, NodeRole::Gateway);
        let res = demo
            .send(request(
                Method::GET,
                &format!("/v1/nodes/{ap}"),
                None,
                Some(&demo.cookie),
            ))
            .await;
        assert_eq!(json(res).await["node"]["role"], "gateway");
        let audit = demo.store.audit_log(10).await.unwrap();
        let roles: Vec<_> = audit
            .iter()
            .filter(|e| e.action == AuditAction::NodeRole)
            .filter_map(|e| e.detail.clone())
            .collect();
        assert_eq!(roles.len(), 2, "{roles:?}");
        assert!(roles.iter().any(|d| d.ends_with("access point to gateway")));
        // The same role again is no change, and no audit entry.
        let res = send(ap, json!({"role": "gateway"})).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(demo.store.audit_log(10).await.unwrap().len(), audit.len());
    }

    #[tokio::test]
    async fn bad_requests_and_unknown_nodes_are_refused() {
        let demo = Demo::start().await;
        let send = |uri: &str, body: serde_json::Value, cookie: Option<&str>| {
            demo.send(request(Method::PUT, uri, Some(&body), cookie))
        };
        let cookie = Some(demo.cookie.as_str());
        let uri = format!("/v1/nodes/{}/role", demo.ids[1]);
        let res = send(&uri, json!({"role": "router"}), cookie).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send("/v1/nodes/x/role", json!({"role": "ap"}), cookie).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send("/v1/nodes/999/role", json!({"role": "ap"}), cookie).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = send(&uri, json!({"role": "ap"}), None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}

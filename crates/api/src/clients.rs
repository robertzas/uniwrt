//! The client routes, with a session (features/observability.md, Clients): `GET
//! /api/v1/clients` is every client the controller knows (the WS topic `clients` keeps it
//! current), `GET /api/v1/clients/{mac}` one client with its last hour of samples and last day
//! of sessions ([`ClientDetail`], the `client:<mac>` topic's snapshot), `GET
//! /api/v1/clients/{mac}/sessions?since=<ms>` its sessions since a time, and `PATCH
//! /api/v1/clients/{mac}` renames it (admins; audited).

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_clients::{Client as ClientView, ClientSample};
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    client::{ClientSession, alias},
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, ClientRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// How far back a client's detail goes for sessions.
pub(crate) const SESSIONS_FOR: Duration = Duration::from_hours(24);
/// The furthest back `sessions?since=` goes: as long as sessions are kept.
const SESSIONS_KEPT: Duration = Duration::from_hours(30 * 24);

/// One client for its page: as it is now, its recent samples and its sessions.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ClientDetail {
    pub client: Arc<ClientView>,
    /// The last hour, oldest first.
    pub samples: Vec<ClientSample>,
    /// The sessions of the last day, oldest first; the last may go on.
    pub sessions: Vec<ClientSession>,
}

/// What `PATCH /api/v1/clients/{mac}` changes.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ClientPatch {
    /// The new alias; blank or `null` takes it away.
    pub alias: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SessionsQuery {
    since: Option<i64>,
}

pub(crate) async fn list(State(api): State<Api>, _: CurrentUser) -> Json<Vec<Arc<ClientView>>> {
    Json(api.fleet().clients())
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    mac: Path<String>,
) -> ApiResult<Json<ClientDetail>> {
    let mac = parse_mac(&mac)?;
    detail(&api, mac)
        .await?
        .map(Json)
        .ok_or_else(|| unknown(mac))
}

pub(crate) async fn sessions(
    State(api): State<Api>,
    _: CurrentUser,
    mac: Path<String>,
    query: Result<Query<SessionsQuery>, QueryRejection>,
) -> ApiResult<Json<Vec<ClientSession>>> {
    let mac = parse_mac(&mac)?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let now = api.now();
    let since = query
        .since
        .map_or_else(|| now.saturating_sub(SESSIONS_FOR), Timestamp::from_unix_ms);
    let since = since.max(now.saturating_sub(SESSIONS_KEPT));
    let live = api.fleet().client(mac).ok_or_else(|| unknown(mac))?;
    Ok(Json(merged(&api, mac, since, live.session).await?))
}

pub(crate) async fn patch(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Path<String>,
    body: Result<Json<ClientPatch>, JsonRejection>,
) -> ApiResult<Json<Arc<ClientView>>> {
    let mac = parse_mac(&mac)?;
    let Json(body) = body?;
    if user.user.role != Role::Admin {
        return Err(ApiError::Forbidden("Only admins rename clients".to_owned()));
    }
    let alias = alias(body.alias.as_deref().unwrap_or_default())
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;
    let record = api
        .fleet()
        .rename_client(mac, alias.clone())
        .ok_or_else(|| unknown(mac))?;
    api.store().save_alias(record).await?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::RenameClient,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(match &alias {
                Some(alias) => format!("{mac} as {alias:?}"),
                None => format!("{mac}, alias removed"),
            }),
        })
        .await?;
    let view = api.fleet().client(mac).ok_or_else(|| unknown(mac))?;
    Ok(Json(view.client))
}

/// A client's detail; `None` for one the engine does not know.
pub(crate) async fn detail(api: &Api, mac: MacAddr) -> ApiResult<Option<ClientDetail>> {
    let Some(live) = api.fleet().client(mac) else {
        return Ok(None);
    };
    let since = api.now().saturating_sub(SESSIONS_FOR);
    let sessions = merged(api, mac, since, live.session).await?;
    Ok(Some(ClientDetail {
        client: live.client,
        samples: live.samples,
        sessions,
    }))
}

/// The stored sessions since `since`, with the open one as the engine has it (the store may
/// not have it yet).
async fn merged(
    api: &Api,
    mac: MacAddr,
    since: Timestamp,
    open: Option<ClientSession>,
) -> ApiResult<Vec<ClientSession>> {
    let mut sessions = api.store().client_sessions(mac, since).await?;
    if let Some(open) = open {
        sessions.retain(|session| session.start != open.start);
        sessions.retain(|session| session.end.is_some());
        sessions.push(open);
        sessions.sort_by_key(|session| session.start);
    }
    Ok(sessions)
}

pub(crate) fn parse_mac(text: &str) -> ApiResult<MacAddr> {
    text.parse()
        .map_err(|_| ApiError::BadRequest(format!("{text:?} is not a MAC address")))
}

fn unknown(mac: MacAddr) -> ApiError {
    ApiError::NotFound(format!("There is no client {mac}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::{
        body::Body,
        extract::Request,
        http::{Method, StatusCode},
    };
    use serde_json::{Value, json};
    use tokio::time::Instant;
    use uniwrt_store::AuditRepo as _;

    use crate::testing::{Demo, json, request};

    /// The demo's clients, once the fleet placed most of them.
    async fn clients(demo: &Demo) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let res = demo.send(demo.get("/v1/clients")).await;
            assert_eq!(res.status(), StatusCode::OK);
            let clients = json(res).await.as_array().unwrap().clone();
            if clients
                .iter()
                .filter(|client| client["online"] == true)
                .count()
                >= 15
            {
                return clients;
            }
            assert!(Instant::now() < deadline, "the clients never showed up");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn patch(demo: &Demo, mac: &str, body: &Value) -> Request<Body> {
        request(
            Method::PATCH,
            &format!("/v1/clients/{mac}"),
            Some(body),
            Some(&demo.cookie),
        )
    }

    #[tokio::test]
    async fn the_demo_clients_are_listed_detailed_and_renamed() {
        let demo = Demo::start().await;
        let clients = clients(&demo).await;
        let wifi = clients
            .iter()
            .find(|client| client["online"] == true && client["connection"]["medium"] == "wifi")
            .unwrap();
        assert!(wifi["experience"]["score"].is_number(), "{wifi}");
        assert!(wifi["connection"]["signal_dbm"].is_number());
        assert!(clients.iter().any(|client| client["vendor"].is_string()));
        let mac = wifi["mac"].as_str().unwrap();

        let res = demo.send(demo.get(&format!("/v1/clients/{mac}"))).await;
        assert_eq!(res.status(), StatusCode::OK);
        let detail = json(res).await;
        assert_eq!(detail["client"]["mac"], mac);
        let sessions = detail["sessions"].as_array().unwrap();
        assert_eq!(sessions.last().unwrap()["end"], Value::Null);

        let res = demo
            .send(patch(&demo, mac, &json!({ "alias": "  Kitchen tablet " })))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(json(res).await["alias"], "Kitchen tablet");
        let audit = demo.store.audit_log(1).await.unwrap();
        assert_eq!(
            audit[0].detail.as_deref(),
            Some(&*format!("{mac} as \"Kitchen tablet\""))
        );
        let res = demo
            .send(patch(&demo, mac, &json!({ "alias": null })))
            .await;
        assert_eq!(json(res).await["alias"], Value::Null);

        let res = demo
            .send(patch(&demo, mac, &json!({ "alias": "x".repeat(65) })))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = demo
            .send(patch(&demo, "02:00:00:00:00:fe", &json!({ "alias": "x" })))
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let res = demo.send(demo.get("/v1/clients/nope")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        let res = demo
            .send(demo.get(&format!("/v1/clients/{mac}/sessions?since=0")))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        let sessions = json(res).await;
        assert_eq!(sessions.as_array().unwrap().last().unwrap()["mac"], mac);
    }
}

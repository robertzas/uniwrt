//! The client actions, for admins (features/wifi.md, Client actions; D-084): `POST
//! /api/v1/clients/{mac}/kick` deauthenticates a client on every managed access point and keeps
//! it off them for `ban_secs` ([`ClientKick`], answered with a [`KickOutcome`]); `PUT` and
//! `DELETE /api/v1/clients/{mac}/block` block and unblock it (the client's view answers). Each is
//! audited; the engine announces it as an event.

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_clients::Client as ClientView;
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_engine::{Bans, BansError, Kicked};
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::{AuditRepo as _, ClientRepo as _};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, clients::parse_mac};

/// What `POST /api/v1/clients/{mac}/kick` asks.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ClientKick {
    /// How long it is kept off the managed access points, up to a day; 0 lets it straight back.
    pub ban_secs: u32,
}

/// How a kick went.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct KickOutcome {
    /// The managed nodes asked.
    pub nodes: u32,
    /// The access points it was deauthenticated from; none when it wasn't connected to one.
    pub from: Vec<KickedNode>,
    pub failed: Vec<KickRefusal>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct KickedNode {
    pub node: NodeId,
    pub ifname: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct KickRefusal {
    pub node: NodeId,
    pub error: String,
}

impl From<Kicked> for KickOutcome {
    fn from(kicked: Kicked) -> Self {
        Self {
            nodes: kicked.nodes,
            from: kicked
                .from
                .into_iter()
                .map(|f| KickedNode {
                    node: f.node,
                    ifname: f.ifname,
                })
                .collect(),
            failed: kicked
                .failed
                .into_iter()
                .map(|f| KickRefusal {
                    node: f.node,
                    error: f.error,
                })
                .collect(),
        }
    }
}

fn admin(user: &CurrentUser, what: &str) -> ApiResult<()> {
    if user.user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("Only admins {what} clients")))
    }
}

/// A client the engine knows, from the path.
fn known(api: &Api, mac: &str) -> ApiResult<MacAddr> {
    let mac = parse_mac(mac)?;
    api.fleet()
        .client(mac)
        .map(|_| mac)
        .ok_or_else(|| ApiError::NotFound(format!("There is no client {mac}")))
}

async fn audit(
    api: &Api,
    user: &CurrentUser,
    client: &Client,
    action: AuditAction,
    detail: String,
) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

pub(crate) async fn kick(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Path<String>,
    body: Result<Json<ClientKick>, JsonRejection>,
) -> ApiResult<Json<KickOutcome>> {
    let Json(body) = body?;
    admin(&user, "kick")?;
    let mac = known(&api, &mac)?;
    let ban = Duration::from_secs(body.ban_secs.into());
    let by = Some(user.user.username.clone());
    let kicked = api
        .bans()?
        .kick(mac, ban, by)
        .await
        .map_err(|err| match err {
            BansError::TooLong { .. } => ApiError::BadRequest(format!(
                "A kick keeps a client off for at most {} s",
                Bans::MAX_KICK.as_secs()
            )),
            BansError::Stopped => ApiError::Unavailable("Client bans are not running".to_owned()),
        })?;
    let detail = format!("{mac}, kept off for {} s", body.ban_secs);
    audit(&api, &user, &client, AuditAction::KickClient, detail).await?;
    Ok(Json(kicked.into()))
}

pub(crate) async fn block(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Path<String>,
) -> ApiResult<Json<Arc<ClientView>>> {
    set_block(api, user, client, &mac, true).await
}

pub(crate) async fn unblock(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Path<String>,
) -> ApiResult<Json<Arc<ClientView>>> {
    set_block(api, user, client, &mac, false).await
}

async fn set_block(
    api: Api,
    user: CurrentUser,
    client: Client,
    mac: &str,
    blocked: bool,
) -> ApiResult<Json<Arc<ClientView>>> {
    admin(&user, if blocked { "block" } else { "unblock" })?;
    let mac = known(&api, mac)?;
    let by = Some(user.user.username.clone());
    let unknown = || ApiError::NotFound(format!("There is no client {mac}"));
    let record = api
        .fleet()
        .block_client(mac, blocked, by)
        .ok_or_else(unknown)?;
    api.store().save_choices(record).await?;
    let detail = format!("{mac} {}", if blocked { "blocked" } else { "unblocked" });
    audit(&api, &user, &client, AuditAction::BlockClient, detail).await?;
    let view = api.fleet().client(mac).ok_or_else(unknown)?;
    Ok(Json(view.client))
}

#[cfg(test)]
mod tests;

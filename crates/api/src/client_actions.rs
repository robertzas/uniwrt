//! The client actions, for admins (features/wifi.md, Client actions; D-084): `POST
//! /api/v1/clients/{mac}/kick` deauthenticates a client on every managed access point and keeps
//! it off them for `ban_secs` ([`ClientKick`], answered with a [`KickOutcome`]); `PUT` and
//! `DELETE /api/v1/clients/{mac}/block` block and unblock it (the client's view answers). Each is
//! audited; the engine announces it as an event. `POST /api/v1/clients/{mac}/wake` sends it a
//! Wake-on-LAN packet from the gateway ([`WakeOutcome`]; D-137), installing `etherwake` there
//! first when the gateway is managed and lacks it; audited.

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_clients::Client as ClientView;
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_engine::{Bans, BansError, Kicked, WakeError};
use uniwrt_model::{
    node::{NodeMode, NodeRole},
    user::{AuditAction, AuditEntry, Role},
};
use uniwrt_store::{AuditRepo as _, ClientRepo as _, NodeRepo as _};

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

/// Where `POST /api/v1/clients/{mac}/wake` sent the packet.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct WakeOutcome {
    /// The gateway.
    pub node: NodeId,
    /// Its devices the packet went out on, such as `br-lan`.
    pub devices: Vec<String>,
    /// Whether `etherwake` was installed on the gateway for it.
    pub installed: bool,
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

pub(crate) async fn wake(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Path<String>,
) -> ApiResult<Json<WakeOutcome>> {
    admin(&user, "wake")?;
    let mac = parse_mac(&mac)?;
    let ips = api
        .fleet()
        .client(mac)
        .ok_or_else(|| ApiError::NotFound(format!("There is no client {mac}")))?
        .client
        .ips
        .clone();
    let gateway = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.role == NodeRole::Gateway)
        .ok_or_else(|| ApiError::Conflict("No gateway is adopted".to_owned()))?;
    if api.jobs().holds(gateway.id) {
        return Err(ApiError::Conflict(format!(
            "A job is running on {}: wake the client once it ends",
            gateway.name
        )));
    }
    let managed = gateway.mode == NodeMode::Managed;
    let woken = uniwrt_engine::wake(api.fleet(), gateway.id, managed, mac, &ips)
        .await
        .map_err(|err| match err {
            WakeError::Offline => ApiError::Conflict(format!("{} is offline", gateway.name)),
            WakeError::NoNetwork => ApiError::Conflict(format!(
                "{} serves no network to send the packet on",
                gateway.name
            )),
            WakeError::NoSender => ApiError::Conflict(format!(
                "{} lacks etherwake, and it is in observe mode: turn its management on and \
                 the controller installs it",
                gateway.name
            )),
            WakeError::Failed(why) => ApiError::BadGateway(why),
        })?;
    let detail = format!(
        "{mac} from {} on {}{}",
        gateway.name,
        woken.devices.join(", "),
        if woken.installed {
            ", etherwake installed"
        } else {
            ""
        }
    );
    audit(&api, &user, &client, AuditAction::WakeClient, detail).await?;
    Ok(Json(WakeOutcome {
        node: woken.node,
        devices: woken.devices,
        installed: woken.installed,
    }))
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

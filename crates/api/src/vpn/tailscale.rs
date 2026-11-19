//! The gateway's Tailscale (features/network.md, VPN; D-140), with a session:
//!
//! - `GET /api/v1/vpn/tailscale` ([`TailscaleStatus`]): the daemon as the gateway's `services`
//!   set reports it, whether it holds the site's prefs (the engine's `TailscaleSync` sets them),
//!   and which of the site's routes the tailnet's admin hasn't approved yet. Any user: it holds no
//!   login URL.
//!
//! - `POST /api/v1/vpn/tailscale/login` (admins; audited): starts an interactive login on the
//!   gateway with the site's prefs and answers the login URL `tailscaled` hands out
//!   ([`TailscaleLogin`]), or the one already waiting; a gateway logged in gets none. Whoever
//!   opens the URL joins the gateway to their tailnet, so it never goes into the facts. 409 while
//!   the site has Tailscale off, without a managed gateway that runs it (apply first), or while a
//!   job holds the gateway; 502 with the CLI's words when the gateway refuses.
//!
//! - `POST /api/v1/vpn/tailscale/logout` (admins; audited): logs the gateway out; its prefs
//!   stay. With an auth key in the site, the engine logs it in again by itself.

use axum::{Json, extract::State};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    facts::{NodeFacts, TailscaleFacts, TailscaleState},
    node::{Node, NodeMode, NodeRole},
    site::{SiteConfig, vpn::tailscale::Prefs},
    status::NodeState,
    user::AuditAction,
};
use uniwrt_node::proto::ops::{Tailscale, TailscaleAction, TailscaleAnswer};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    site::no_site,
};

/// Tailscale on the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TailscaleStatus {
    /// Whether the gateway answers; its last facts are shown while it doesn't.
    pub online: bool,
    /// When the gateway's facts were collected; `None` without a gateway or its facts.
    pub collected_at: Option<Timestamp>,
    /// `tailscaled` as the gateway reports it; `None` without the package.
    pub daemon: Option<TailscaleFacts>,
    /// The daemon holds the site's prefs (routes, exit node, name): `false` until the
    /// controller sets them after a save, and while the site has no Tailscale.
    pub prefs_applied: bool,
    /// What the site advertises that the tailnet's admin hasn't approved yet: subnet routes, and
    /// `0.0.0.0/0` for the exit node. Empty unless the daemon runs.
    pub unapproved_routes: Vec<String>,
}

/// A login started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TailscaleLogin {
    /// The daemon's state afterwards.
    pub state: TailscaleState,
    /// The URL that logs the gateway in, for the admin to open; `None` when it needs none.
    pub auth_url: Option<String>,
}

impl From<TailscaleAnswer> for TailscaleLogin {
    fn from(answer: TailscaleAnswer) -> Self {
        Self {
            state: answer.state,
            auth_url: answer.auth_url,
        }
    }
}

pub(crate) async fn status(
    State(api): State<Api>,
    _: CurrentUser,
) -> ApiResult<Json<TailscaleStatus>> {
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let view = gateway(&api)
        .await?
        .and_then(|node| api.fleet().node(node.id));
    let online = view
        .as_ref()
        .is_some_and(|view| view.status.state != NodeState::Offline);
    let facts = view.and_then(|view| view.facts);
    Ok(Json(report(&head.config, facts.as_deref(), online)))
}

/// The site's Tailscale against the gateway's `facts`.
pub(crate) fn report(
    config: &SiteConfig,
    facts: Option<&NodeFacts>,
    online: bool,
) -> TailscaleStatus {
    let daemon = facts.and_then(|facts| facts.services.tailscale.as_deref());
    let prefs = config.vpn.tailscale.as_ref().map(|t| t.prefs(config));
    let prefs_applied = matches!(
        (&prefs, daemon.and_then(|d| d.prefs.as_ref())),
        (Some(want), Some(held)) if want.held_by(held)
    );
    let unapproved_routes = match (&prefs, daemon) {
        (Some(want), Some(daemon)) if daemon.state == Some(TailscaleState::Running) => want
            .routes
            .iter()
            .cloned()
            .chain(want.exit_node.then(|| "0.0.0.0/0".to_owned()))
            .filter(|route| !daemon.approved_routes.contains(route))
            .collect(),
        _ => Vec::new(),
    };
    TailscaleStatus {
        online,
        collected_at: facts.map(|facts| facts.collected_at),
        daemon: daemon.cloned(),
        prefs_applied,
        unapproved_routes,
    }
}

pub(crate) async fn login(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<Json<TailscaleLogin>> {
    admin(&user, "log Tailscale in")?;
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let prefs: Prefs = match &head.config.vpn.tailscale {
        Some(tailscale) if tailscale.enabled => tailscale.prefs(&head.config),
        _ => {
            return Err(ApiError::Conflict(
                "Tailscale is off in the site".to_owned(),
            ));
        }
    };
    let node = ready(&api).await?;
    let answer = call(&api, &node, Tailscale::new(TailscaleAction::Login, prefs)).await?;
    let detail = match answer.auth_url {
        Some(_) => format!("started a Tailscale login on {}", node.name),
        None => format!(
            "asked for a Tailscale login on {}, which needs none",
            node.name
        ),
    };
    audit(&api, &user, &client, AuditAction::Tailscale, detail).await?;
    Ok(Json(answer.into()))
}

pub(crate) async fn logout(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<Json<TailscaleLogin>> {
    admin(&user, "log Tailscale out")?;
    let node = ready(&api).await?;
    let answer = call(&api, &node, Tailscale::logout()).await?;
    let detail = format!("logged Tailscale out on {}", node.name);
    audit(&api, &user, &client, AuditAction::Tailscale, detail).await?;
    Ok(Json(answer.into()))
}

async fn gateway(api: &Api) -> ApiResult<Option<Node>> {
    Ok(api
        .store()
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.role == NodeRole::Gateway))
}

/// The gateway, managed, online, free of jobs and running Tailscale.
async fn ready(api: &Api) -> ApiResult<Node> {
    let node = gateway(api)
        .await?
        .ok_or_else(|| ApiError::Conflict("No gateway is adopted".to_owned()))?;
    let name = &node.name;
    if node.mode != NodeMode::Managed {
        return Err(ApiError::Conflict(format!(
            "{name} is in observe mode: turn its management on first"
        )));
    }
    if api.jobs().holds(node.id) {
        return Err(ApiError::Conflict(format!(
            "A job is running on {name}: try again once it ends"
        )));
    }
    let view = api
        .fleet()
        .node(node.id)
        .filter(|view| view.status.state != NodeState::Offline)
        .ok_or_else(|| ApiError::Conflict(format!("{name} is offline")))?;
    let runs = view
        .facts
        .is_some_and(|facts| facts.services.tailscale.is_some());
    if !runs {
        return Err(ApiError::Conflict(format!(
            "{name} doesn't run Tailscale yet: apply the site to install it"
        )));
    }
    Ok(node)
}

async fn call(api: &Api, node: &Node, request: Tailscale) -> ApiResult<TailscaleAnswer> {
    api.fleet()
        .tailscale(node.id, request)
        .await
        .map_err(|err| match err {
            uniwrt_node::Error::Remote { msg, .. } => ApiError::BadGateway(msg),
            uniwrt_node::Error::Disconnected { .. } => {
                ApiError::Conflict(format!("{} is offline", node.name))
            }
            other => ApiError::BadGateway(other.to_string()),
        })
}

#[cfg(test)]
#[path = "tailscale_tests.rs"]
mod tests;

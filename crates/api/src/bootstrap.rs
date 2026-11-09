//! First run with a factory-default gateway (features/adoption.md, Path E; D-080), for admins:
//!
//! - `GET /api/v1/bootstrap/gateway` ([`FoundGateways`]): the routers at the controller's default
//!   gateway and at OpenWrt's 192.168.1.1 where dropbear answers, each with its host key (to
//!   confirm), whether root has no password, and then its model and release. Adopting one is
//!   `POST /api/v1/adopt` with an empty password: the job sets a random root password.
//! - `POST /api/v1/nodes/{id}/root-password/reveal` ([`RevealedSecret`]; audited): the root
//!   password the vault keeps for a node (set at adoption, or given and remembered), for
//!   emergencies; 404 when it keeps none.

use axum::{
    Json,
    extract::{Path, State, rejection::PathRejection},
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_jobs::bootstrap::{self, FoundGateway};
use uniwrt_model::user::AuditAction;
use uniwrt_store::{NodeRepo as _, VaultRepo as _};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    jobs::jobs_error,
    site::RevealedSecret,
};

/// Where the kernel lists the controller's routes.
const PROC_NET_ROUTE: &str = "/proc/net/route";

/// The routers that could be the gateway to set up.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FoundGateways {
    /// The addresses looked at, in order: the controller's default gateway, then 192.168.1.1.
    pub candidates: Vec<String>,
    /// The routers found there, in that order.
    pub gateways: Vec<GatewayView>,
}

/// One router: dropbear answered at its address.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GatewayView {
    pub host: String,
    pub port: u16,
    /// OpenSSH form, which `POST /api/v1/adopt` takes back once the user confirmed it.
    pub host_key: String,
    /// SHA-256, as OpenSSH shows it: `SHA256:…`.
    pub fingerprint: String,
    pub algorithm: String,
    /// Root has no password (factory defaults): adopt it with an empty one.
    pub blank: bool,
    /// Known when root has no password.
    pub model: Option<String>,
    pub hostname: Option<String>,
    /// `OpenWrt 24.10.8`.
    pub release: Option<String>,
    /// Why the controller can't manage it, when known.
    pub unsupported: Option<String>,
    /// The node adopted at this address, if any.
    pub adopted: Option<NodeId>,
}

impl From<FoundGateway> for GatewayView {
    fn from(found: FoundGateway) -> Self {
        Self {
            host: found.target.host,
            port: found.target.port,
            host_key: found.host_key.to_openssh().to_owned(),
            fingerprint: found.host_key.fingerprint().to_owned(),
            algorithm: found.host_key.algorithm(),
            blank: found.blank,
            model: found.model,
            hostname: found.hostname,
            release: found.release,
            unsupported: found.unsupported,
            adopted: found.adopted,
        }
    }
}

pub(crate) async fn gateway(
    State(api): State<Api>,
    user: CurrentUser,
) -> ApiResult<Json<FoundGateways>> {
    admin(&user, "set up the gateway")?;
    // Unreadable (not Linux, a sandbox): only OpenWrt's default is looked at.
    let routes = tokio::task::spawn_blocking(|| std::fs::read_to_string(PROC_NET_ROUTE))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let hosts = bootstrap::candidates(bootstrap::default_gateway(&routes));
    let found = api.jobs().find_gateways(&hosts).await.map_err(jobs_error)?;
    Ok(Json(FoundGateways {
        candidates: hosts.iter().map(ToString::to_string).collect(),
        gateways: found.into_iter().map(GatewayView::from).collect(),
    }))
}

pub(crate) async fn reveal_root_password(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Json<RevealedSecret>> {
    admin(&user, "view root passwords")?;
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    let node = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    let value = api
        .vault()?
        .secret(&format!("node/{id}/root_password"))
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "The vault keeps no root password for {}",
                node.name
            ))
        })?;
    let value = String::from_utf8(value.to_vec())
        .map_err(|_| ApiError::Internal(format!("node {id}'s root password is not UTF-8")))?;
    let detail = format!("viewed the root password of {} (node {id})", node.name);
    audit(&api, &user, &client, AuditAction::RevealSecret, detail).await?;
    Ok(Json(RevealedSecret { value }))
}

#[cfg(test)]
mod tests;

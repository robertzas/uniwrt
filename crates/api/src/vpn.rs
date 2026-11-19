//! The WireGuard server's peers (features/network.md, VPN; D-140), with a session:
//!
//! - `POST /api/v1/vpn/wireguard/peers/{id}/config` (admins; audited as a secret revealed): the
//!   peer's configuration in the latest revision ([`PeerConfig`]), a `.conf` that the
//!   WireGuard apps import, with its private and preshared keys from the vault
//!   ([`uniwrt_render::peer_conf`]). A peer that brought its own keypair gets the file without
//!   its private key. 404 for a peer the revision lacks; 409 while the server has no keypair or
//!   its endpoint names a DDNS entry that is gone.
//!
//! - `GET /api/v1/vpn/wireguard/peers` ([`VpnStatus`]): the peers as the gateway's tunnel
//!   reports them (handshake, endpoint, traffic; [`stats`]).
//!
//! The site's saves and imports make the keys the server and its peers lack ([`keys`]).

pub(crate) mod keys;
pub(crate) mod stats;

pub use stats::{VpnPeerStatus, VpnStatus};

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_model::{
    site::{SecretRef, vpn::Peer},
    user::AuditAction,
};
use uniwrt_render::{PeerKeys, peer_conf};
use uniwrt_store::{RevisionRepo as _, VaultRepo as _};
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    site::no_site,
};

/// A peer's configuration file.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, rename = "VpnPeerConfig")]
pub struct PeerConfig {
    /// What to save it as: the peer's id, which `wg-quick` takes as the interface's name.
    pub file_name: String,
    /// The `.conf` text.
    pub conf: String,
    /// The peer brought its own keypair: the file lacks its private key.
    pub own_key: bool,
}

pub(crate) async fn config(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<String>,
) -> ApiResult<Json<PeerConfig>> {
    admin(&user, "view a VPN peer's configuration")?;
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let peer: &Peer = head
        .config
        .vpn
        .wireguard
        .as_ref()
        .and_then(|server| server.peers.iter().find(|peer| peer.id == id))
        .ok_or_else(|| {
            ApiError::NotFound(format!("Revision {} has no VPN peer {id}", head.meta.id))
        })?;
    let private_key = match &peer.private_key {
        Some(reference) => Some(read(&api, reference).await?),
        None => None,
    };
    let preshared_key = match &peer.preshared_key {
        Some(reference) => read(&api, reference).await?,
        None => {
            return Err(ApiError::Conflict(format!(
                "The peer {id} has no preshared key"
            )));
        }
    };
    let keys = PeerKeys {
        private_key: private_key.as_deref().map(String::as_str),
        preshared_key: &preshared_key,
    };
    let conf =
        peer_conf(&head.config, peer, keys).map_err(|issue| ApiError::Conflict(issue.message))?;
    let detail = format!(
        "viewed the configuration of VPN peer {id} of revision {}",
        head.meta.id
    );
    audit(&api, &user, &client, AuditAction::RevealSecret, detail).await?;
    Ok(Json(PeerConfig {
        file_name: format!("{id}.conf"),
        conf,
        own_key: private_key.is_none(),
    }))
}

async fn read(api: &Api, reference: &SecretRef) -> ApiResult<Zeroizing<String>> {
    let value = api
        .vault()?
        .secret(reference.name())
        .await?
        .ok_or_else(|| ApiError::Internal(format!("the vault lacks {}", reference.name())))?;
    std::str::from_utf8(&value)
        .map(|value| Zeroizing::new(value.to_owned()))
        .map_err(|_| ApiError::Internal(format!("{} is not UTF-8", reference.name())))
}

#[cfg(test)]
mod tests;

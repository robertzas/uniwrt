//! `GET /api/v1/vpn/wireguard/peers` ([`VpnStatus`]): each peer of the latest revision with
//! what the gateway's tunnel reports of it (the `services` set's `wg show vpn dump`, matched by
//! public key; D-140): its latest handshake, endpoint and traffic, and whether it is connected.

use std::time::Duration;

use axum::{Json, extract::State};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    facts::{NodeFacts, WireguardPeerFacts},
    node::NodeRole,
    site::vpn::WireGuard,
    status::NodeState,
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _};

use crate::{Api, ApiResult, CurrentUser, site::no_site};

/// WireGuard rejects a session three minutes after its handshake (`REJECT_AFTER_TIME`); a
/// peer that keeps talking makes a new one every two.
pub(crate) const CONNECTED_WITHIN: Duration = Duration::from_secs(180);

/// The tunnel on the gateway and its peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct VpnStatus {
    /// Whether the gateway answers; its last facts are shown while it doesn't.
    pub online: bool,
    /// When the gateway's facts were collected, on the controller's clock; `None` without a
    /// gateway or its facts.
    pub collected_at: Option<Timestamp>,
    /// The tunnel's device is up on the gateway.
    pub running: bool,
    /// The tunnel runs with the revision's server key: `false` until the revision that made
    /// the key is applied (or while the gateway doesn't report its key).
    pub key_applied: bool,
    /// The revision's peers, in its order.
    pub peers: Vec<VpnPeerStatus>,
}

/// One peer: `on_gateway` is `false` (and the rest empty) until the revision that adds it,
/// or its key, is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct VpnPeerStatus {
    pub id: String,
    pub on_gateway: bool,
    /// A handshake within [`CONNECTED_WITHIN`] of the collection.
    pub connected: bool,
    /// Its latest handshake, on the controller's clock.
    pub handshake_at: Option<Timestamp>,
    /// Where its packets last came from.
    pub endpoint: Option<String>,
    /// Bytes from the peer and to it since the tunnel came up.
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

pub(crate) async fn peers(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<VpnStatus>> {
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let gateway = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.role == NodeRole::Gateway);
    let view = gateway.and_then(|node| api.fleet().node(node.id));
    let online = view
        .as_ref()
        .is_some_and(|view| view.status.state != NodeState::Offline);
    let facts = view.and_then(|view| view.facts);
    Ok(Json(status(
        head.config.vpn.wireguard.as_ref(),
        facts.as_deref(),
        online,
    )))
}

/// The revision's `server` against the gateway's `facts`.
pub(crate) fn status(
    server: Option<&WireGuard>,
    facts: Option<&NodeFacts>,
    online: bool,
) -> VpnStatus {
    let tunnel = facts.and_then(|facts| facts.services.wireguard.as_ref());
    // The node's clock against the controller's, from the `system` set read with the facts.
    let skew_ms = facts.and_then(|facts| {
        let node = facts.system.node_time?;
        Some(facts.collected_at.as_unix_ms() - node.as_unix_ms())
    });
    let peers = server.map_or_else(Vec::new, |server| {
        server
            .peers
            .iter()
            .map(|peer| {
                let seen: Option<&WireguardPeerFacts> = tunnel.and_then(|tunnel| {
                    tunnel
                        .peers
                        .iter()
                        .find(|p| p.public_key == peer.public_key)
                });
                let connected = seen.is_some_and(|seen| {
                    match (seen.handshake_at, tunnel.and_then(|t| t.node_time)) {
                        (Some(at), Some(now)) => {
                            now.saturating_duration_since(at) <= CONNECTED_WITHIN
                        }
                        _ => false,
                    }
                });
                VpnPeerStatus {
                    id: peer.id.clone(),
                    on_gateway: seen.is_some(),
                    connected,
                    handshake_at: seen.and_then(|seen| seen.handshake_at).map(|at| {
                        Timestamp::from_unix_ms(at.as_unix_ms() + skew_ms.unwrap_or_default())
                    }),
                    endpoint: seen.and_then(|seen| seen.endpoint).map(|e| e.to_string()),
                    rx_bytes: seen.map_or(0, |seen| seen.rx_bytes),
                    tx_bytes: seen.map_or(0, |seen| seen.tx_bytes),
                }
            })
            .collect()
    });
    VpnStatus {
        online,
        collected_at: facts.map(|facts| facts.collected_at),
        running: tunnel.is_some(),
        key_applied: matches!(
            (server.and_then(|s| s.public_key.as_deref()), tunnel.and_then(|t| t.public_key.as_deref())),
            (Some(want), Some(have)) if want == have
        ),
        peers,
    }
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;

//! The address plan's suggestions (features/network.md, Address plan; D-077), from the latest
//! revision; nothing is saved:
//!
//! - `GET v1/ipam/network?purpose=<purpose>[&vlan=<id>]`: a new network of that purpose, on the
//!   VLAN given or its usual one, with the /24 whose third octet is the VLAN, its pool and the
//!   policy its purpose seeds ([`Network`]); 409 when no VLAN or /24 is free.
//! - `GET v1/ipam/reservation?ip=<address>[&mac=<mac>]`: the address to reserve for a client
//!   ("Fix IP", [`ReservationSuggestion`]): the reservation the MAC address has already, else
//!   on the network holding `ip`, `ip` itself when free, else one of .32–.99; 404 when no
//!   network serves `ip`, 409 when it has no free address.

use std::net::Ipv4Addr;

use axum::{
    Json,
    extract::{Query, State, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::MacAddr;
use uniwrt_model::site::{
    ipam,
    network::{Network, Purpose},
};
use uniwrt_store::RevisionRepo as _;

use crate::{Api, ApiError, ApiResult, CurrentUser};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkQuery {
    purpose: Purpose,
    vlan: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReservationQuery {
    ip: Ipv4Addr,
    mac: Option<MacAddr>,
}

/// Where to reserve a client's address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ReservationSuggestion {
    /// The network's id.
    pub network: String,
    #[ts(as = "String")]
    pub ip: Ipv4Addr,
    /// The MAC address has this reservation already.
    pub existing: bool,
}

async fn head(api: &Api) -> ApiResult<uniwrt_model::site::SiteConfig> {
    let head = api
        .store()
        .head_revision()
        .await?
        .ok_or_else(|| ApiError::Unavailable("The site has no configuration yet".to_owned()))?;
    Ok(head.config)
}

pub(crate) async fn network(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<NetworkQuery>, QueryRejection>,
) -> ApiResult<Json<Network>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    if query.purpose == uniwrt_model::site::network::Purpose::Default {
        return Err(ApiError::BadRequest(
            "The site has its default network already".to_owned(),
        ));
    }
    if let Some(vlan) = query.vlan
        && !(1..=4094).contains(&vlan)
    {
        return Err(ApiError::BadRequest("A VLAN ID is 1 to 4094".to_owned()));
    }
    let config = head(&api).await?;
    if let Some(other) = query
        .vlan
        .and_then(|vlan| config.networks.iter().find(|n| n.vlan == Some(vlan)))
    {
        return Err(ApiError::Conflict(format!(
            "{} is on VLAN {} already",
            other.name,
            query.vlan.unwrap_or_default()
        )));
    }
    ipam::network(&config, query.purpose, query.vlan)
        .map(Json)
        .ok_or_else(|| {
            ApiError::Conflict(format!(
                "No VLAN or /24 is free in the site's space {}",
                config.addressing.ipv4
            ))
        })
}

pub(crate) async fn reservation(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<ReservationQuery>, QueryRejection>,
) -> ApiResult<Json<ReservationSuggestion>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let config = head(&api).await?;
    let held = query.mac.and_then(|mac| {
        config.networks.iter().find_map(|network| {
            let dhcp = network.ipv4.as_ref()?.dhcp.as_ref()?;
            let reservation = dhcp.reservations.iter().find(|r| r.mac == mac)?;
            Some(ReservationSuggestion {
                network: network.id.clone(),
                ip: reservation.ip,
                existing: true,
            })
        })
    });
    if let Some(held) = held {
        return Ok(Json(held));
    }
    let network = config
        .networks
        .iter()
        .find(|n| {
            n.ipv4
                .as_ref()
                .is_some_and(|v4| v4.address.trunc().contains(&query.ip))
        })
        .ok_or_else(|| ApiError::NotFound(format!("No network serves {}", query.ip)))?;
    let ip = ipam::reservation(network, Some(query.ip)).ok_or_else(|| {
        ApiError::Conflict(format!("{} has no free address to reserve", network.name))
    })?;
    Ok(Json(ReservationSuggestion {
        network: network.id.clone(),
        ip,
        existing: false,
    }))
}

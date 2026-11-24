//! The Connection Doctor's route (features/observability.md, Troubleshooting assistants;
//! D-144): `GET v1/clients/{mac}/doctor?hours=<1–168>` gathers one client's events, the log
//! lines that name it, its DNS counts and failed queries and the WLANs it may use over the last
//! `hours` (24 by default), and answers what [`uniwrt_clients::doctor::diagnose`] makes of
//! them ([`Diagnosis`]). For any signed-in user, as the client's page is.

use std::{collections::BTreeSet, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State, rejection::QueryRejection},
};
use serde::Deserialize;
use uniwrt_clients::doctor::{Diagnosis, DnsInput, Input, WlanInfo, diagnose};
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    dns_queries::Outcome,
    events::EventFilter,
    facts::WifiIfaceMode,
    log::LogFilter,
    site::{SiteConfig, steering::SteeringProfile},
};
use uniwrt_store::{
    DnsClient, DnsQueryFilter, DnsQueryRepo as _, EventRepo as _, LogRepo as _, RevisionRepo as _,
};

use crate::{Api, ApiError, ApiResult, CurrentUser, clients::parse_mac};

const HOURS: u64 = 24;
const MAX_HOURS: u64 = 168;
/// The most events, log lines and failed queries read.
const EVENTS_MAX: usize = 2_000;
const LOGS_MAX: usize = 2_000;
const FAILURES_MAX: usize = 1_000;

#[derive(Debug, Deserialize)]
pub(crate) struct DoctorQuery {
    hours: Option<u64>,
}

pub(crate) async fn get(
    State(api): State<Api>,
    _: CurrentUser,
    mac: Path<String>,
    query: Result<Query<DoctorQuery>, QueryRejection>,
) -> ApiResult<Json<Diagnosis>> {
    let mac = parse_mac(&mac)?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let hours = query.hours.unwrap_or(HOURS);
    if !(1..=MAX_HOURS).contains(&hours) {
        return Err(ApiError::BadRequest(format!(
            "hours is between 1 and {MAX_HOURS}"
        )));
    }
    let live = api
        .fleet()
        .client(mac)
        .ok_or_else(|| ApiError::NotFound(format!("There is no client {mac}")))?;
    let now = api.now();
    let from = now.saturating_sub(Duration::from_hours(hours));
    let store = api.store();
    let events = store.events(
        EventFilter {
            clients: vec![mac],
            from: Some(from),
            ..EventFilter::default()
        },
        None,
        EVENTS_MAX,
    );
    let logs = store.logs(
        LogFilter {
            text: Some(format!("\"{mac}\"")),
            from: Some(from),
            ..LogFilter::default()
        },
        None,
        LOGS_MAX,
    );
    let (events, logs, head) = tokio::try_join!(events, logs, store.head_revision())?;
    let config = head.map(|head| head.config);
    let dns = dns(&api, mac, from, config.as_ref()).await?;
    let wlans = config
        .as_ref()
        .map_or_else(Vec::new, |config| wlans(&api, config));
    Ok(Json(diagnose(&Input {
        now,
        from,
        client: &live.client,
        samples: &live.samples,
        events: &events,
        logs: &logs,
        dns: &dns,
        wlans: &wlans,
    })))
}

/// The client's DNS counts over the span, while the gateway logs queries (D-133).
async fn dns(
    api: &Api,
    mac: MacAddr,
    from: Timestamp,
    config: Option<&SiteConfig>,
) -> ApiResult<DnsInput> {
    let logged = config.is_some_and(|config| {
        let blocking = &config.services.blocking;
        blocking.enabled && blocking.stats
    });
    if !logged {
        return Ok(DnsInput::default());
    }
    let store = api.store();
    let key = mac.to_string();
    let hours = store.dns_hours(from);
    let failures = store.dns_queries(
        DnsQueryFilter {
            since: Some(from),
            client: Some(DnsClient::Mac(mac)),
            outcome: Some(Outcome::Failed),
            domain: None,
        },
        None,
        FAILURES_MAX,
    );
    let (hours, failures) = tokio::try_join!(hours, failures)?;
    let (queries, blocked) = hours
        .iter()
        .filter(|hour| hour.client == key)
        .fold((0, 0), |(queries, blocked), hour| {
            (queries + hour.queries, blocked + hour.blocked)
        });
    let failures: Vec<_> = failures.into_iter().map(|(_, query)| query).collect();
    Ok(DnsInput {
        logged,
        // The hours start at the hour holding `from`, so the failures may outnumber the
        // queries counted.
        queries: queries.max(failures.len() as u64),
        blocked,
        failed: failures.len() as u64,
        failures,
    })
}

/// The site's WLANs that are on, with how many nodes broadcast each.
fn wlans(api: &Api, config: &SiteConfig) -> Vec<WlanInfo> {
    let nodes = api.fleet().all();
    let steering = config.steering.profile != SteeringProfile::Off;
    config
        .wlans
        .iter()
        .filter(|wlan| wlan.enabled)
        .map(|wlan| {
            let access_points: BTreeSet<_> = nodes
                .iter()
                .filter(|(_, view)| {
                    view.facts.as_ref().is_some_and(|facts| {
                        facts
                            .radios
                            .iter()
                            .flat_map(|radio| &radio.ifaces)
                            .any(|iface| {
                                iface.mode == WifiIfaceMode::Ap
                                    && iface.up
                                    && iface.ssid.as_deref() == Some(wlan.ssid.as_str())
                            })
                    })
                })
                .map(|(node, _)| *node)
                .collect();
            WlanInfo {
                ssid: wlan.ssid.clone(),
                fast_roaming: wlan.fast_roams(),
                steering: steering && wlan.roaming.steer,
                bands: wlan.bands.clone(),
                access_points: access_points.len(),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;

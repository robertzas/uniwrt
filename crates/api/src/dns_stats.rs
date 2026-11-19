//! DNS query statistics (features/network.md, Query statistics; D-133), with a session:
//!
//! - `GET /api/v1/services/dns/stats?range=24h|7d|30d` ([`DnsStats`]): the range's totals, its
//!   hourly series summed over clients, each client's totals (named as `/clients` names them),
//!   and the top domains and top blocked domains of the last 24 h (the queries are kept no
//!   longer).
//! - `GET /api/v1/services/dns/queries?client=&outcome=&domain=&before=<time>:<id>&limit=<n>`
//!   ([`DnsQueryPage`]): the stored queries, newest first, a page at a time; `client` is a MAC,
//!   or the address of a client no MAC was known for; `domain` is text the domain contains.

use std::{collections::HashMap, net::IpAddr, time::Duration};

use axum::{
    Json,
    extract::{Query, State, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::dns_queries::{DnsHourTotal, DnsQuery, DomainCount, Outcome};
use uniwrt_store::{DnsClient, DnsQueryFilter, DnsQueryRepo as _};

use crate::{Api, ApiError, ApiResult, CurrentUser};

/// The span of the statistics' totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[ts(export)]
pub enum DnsRange {
    #[default]
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

impl DnsRange {
    fn span(self) -> Duration {
        Duration::from_hours(match self {
            Self::Day => 24,
            Self::Week => 7 * 24,
            Self::Month => 30 * 24,
        })
    }
}

/// The statistics of a range.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DnsStats {
    pub range: DnsRange,
    /// The range's start; its counts are from the hour holding it.
    pub since: Timestamp,
    pub queries: u64,
    pub blocked: u64,
    /// The hours with queries, oldest first.
    pub hours: Vec<DnsHourTotal>,
    /// Most queries first.
    pub clients: Vec<DnsClientStats>,
    /// The start of the top domains' span: the last 24 h, whatever the range.
    pub domains_since: Timestamp,
    /// The domains answered most (not blocked).
    pub top_domains: Vec<DomainCount>,
    pub top_blocked: Vec<DomainCount>,
}

/// One client's queries over the range.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DnsClientStats {
    /// Its MAC, or its address where no MAC was known (the `client` of the query pages).
    pub client: String,
    /// The MAC, when `client` is one.
    pub mac: Option<String>,
    /// Its alias or DHCP hostname, as `/clients` has it.
    pub name: Option<String>,
    pub queries: u64,
    pub blocked: u64,
}

/// One page of queries, newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DnsQueryPage {
    pub queries: Vec<DnsQuery>,
    /// The `before` of the next page, when there may be one.
    pub next: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StatsQuery {
    #[serde(default)]
    range: DnsRange,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct QueriesQuery {
    client: Option<String>,
    outcome: Option<Outcome>,
    domain: Option<String>,
    before: Option<String>,
    limit: Option<usize>,
}

/// How long the queries are kept: the top domains' span.
const DOMAINS_SPAN: Duration = Duration::from_hours(24);
const TOP: usize = 20;
const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1_000;

pub(crate) async fn stats(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<StatsQuery>, QueryRejection>,
) -> ApiResult<Json<DnsStats>> {
    let Query(StatsQuery { range }) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let now = api.now();
    let since = now.saturating_sub(range.span());
    let domains_since = now.saturating_sub(DOMAINS_SPAN);
    let store = api.store();
    let (totals, top_domains, top_blocked) = tokio::try_join!(
        store.dns_totals(since),
        store.dns_top_domains(domains_since, false, TOP),
        store.dns_top_domains(domains_since, true, TOP),
    )?;
    let names = Names::new(&api);
    let clients = totals
        .clients
        .into_iter()
        .map(|total| {
            let mac = total.client.parse::<MacAddr>().ok();
            DnsClientStats {
                mac: mac.map(|mac| mac.to_string()),
                name: names.name(&total.client, mac),
                client: total.client,
                queries: total.queries,
                blocked: total.blocked,
            }
        })
        .collect();
    Ok(Json(DnsStats {
        range,
        since,
        queries: totals.hours.iter().map(|h| h.queries).sum(),
        blocked: totals.hours.iter().map(|h| h.blocked).sum(),
        hours: totals.hours,
        clients,
        domains_since,
        top_domains,
        top_blocked,
    }))
}

pub(crate) async fn queries(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<QueriesQuery>, QueryRejection>,
) -> ApiResult<Json<DnsQueryPage>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "`limit` is from 1 to {MAX_LIMIT}"
        )));
    }
    let filter = DnsQueryFilter {
        since: None,
        client: query.client.as_deref().map(client).transpose()?,
        outcome: query.outcome,
        domain: query.domain.map(|text| text.trim().to_ascii_lowercase()),
    };
    let before = query.before.as_deref().map(cursor).transpose()?;
    let rows = api.store().dns_queries(filter, before, limit).await?;
    let next = (rows.len() == limit)
        .then(|| rows.last())
        .flatten()
        .map(|(id, last)| format!("{}:{id}", last.at.as_unix_ms()));
    let queries = rows.into_iter().map(|(_, query)| query).collect();
    Ok(Json(DnsQueryPage { queries, next }))
}

/// A MAC, or an address.
fn client(text: &str) -> ApiResult<DnsClient> {
    text.parse::<MacAddr>()
        .map(DnsClient::Mac)
        .or_else(|_| text.parse::<IpAddr>().map(DnsClient::Address))
        .map_err(|_| ApiError::BadRequest(format!("{text:?} is neither a MAC nor an address")))
}

/// `<time>:<id>`, as a page's `next` gives it.
fn cursor(text: &str) -> ApiResult<(Timestamp, i64)> {
    text.split_once(':')
        .and_then(|(time, id)| {
            Some((
                Timestamp::from_unix_ms(time.parse().ok()?),
                id.parse().ok()?,
            ))
        })
        .ok_or_else(|| ApiError::BadRequest(format!("{text:?} is not a page cursor")))
}

/// The clients' names, by MAC and by address.
struct Names {
    by_mac: HashMap<MacAddr, String>,
    by_address: HashMap<IpAddr, String>,
}

impl Names {
    fn new(api: &Api) -> Self {
        let mut names = Self {
            by_mac: HashMap::new(),
            by_address: HashMap::new(),
        };
        for client in api.fleet().clients() {
            let Some(name) = client.alias.clone().or_else(|| client.hostname.clone()) else {
                continue;
            };
            for &ip in &client.ips {
                names.by_address.insert(ip, name.clone());
            }
            names.by_mac.insert(client.mac, name);
        }
        names
    }

    /// A statistics client's name: its MAC's, or the client now holding its address.
    fn name(&self, client: &str, mac: Option<MacAddr>) -> Option<String> {
        match mac {
            Some(mac) => self.by_mac.get(&mac).cloned(),
            None => client
                .parse()
                .ok()
                .and_then(|ip| self.by_address.get(&ip).cloned()),
        }
    }
}

#[cfg(test)]
#[path = "dns_stats_tests.rs"]
mod tests;

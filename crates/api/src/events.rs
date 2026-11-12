//! The events routes, with a session (features/observability.md, Events):
//!
//! - `GET /api/v1/events?<filter>&before=<time>:<id>&limit=<n>`: recorded events, newest first,
//!   a page at a time ([`EventPage`]; `next` is the cursor of the following page).
//!
//! A filter is `categories=<name,…>&kinds=<name,…>&min=<level>&nodes=<id,…>&clients=<mac,…>&
//! from=<ms>&to=<ms>`, every part optional ([`EventFilter`]).

use axum::{
    Json,
    extract::{Query, State, rejection::QueryRejection},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{EventId, MacAddr, NodeId, Timestamp};
use uniwrt_model::events::{EventCategory, EventFilter, EventLevel, EventRecord};
use uniwrt_store::EventRepo as _;

use crate::{Api, ApiError, ApiResult, CurrentUser};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1_000;
const MAX_VALUES: usize = 32;

/// One page of events, newest first.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EventPage {
    pub events: Vec<EventRecord>,
    /// The `before` of the next page, when there may be one.
    pub next: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EventsQuery {
    categories: Option<String>,
    kinds: Option<String>,
    min: Option<String>,
    nodes: Option<String>,
    clients: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    before: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    _: CurrentUser,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> ApiResult<Json<EventPage>> {
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let filter = filter(&query)?;
    let before = query.before.as_deref().map(cursor).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::BadRequest(format!(
            "`limit` is from 1 to {MAX_LIMIT}"
        )));
    }
    let events = api.store().events(filter, before, limit).await?;
    let next = (events.len() == limit)
        .then(|| events.last())
        .flatten()
        .map(|last| format!("{}:{}", last.at.as_unix_ms(), last.id));
    Ok(Json(EventPage { events, next }))
}

fn filter(query: &EventsQuery) -> ApiResult<EventFilter> {
    let parse_all = |value: Option<&str>, what: &str, parse: &dyn Fn(&str) -> bool| {
        let values = list_of(value);
        if values.len() > MAX_VALUES {
            return Err(ApiError::BadRequest(format!(
                "A filter names at most {MAX_VALUES} {what}"
            )));
        }
        match values.iter().find(|value| !parse(value)) {
            Some(bad) => Err(ApiError::BadRequest(format!(
                "{bad:?} is not one of the {what}"
            ))),
            None => Ok(values),
        }
    };
    let categories = parse_all(query.categories.as_deref(), "categories", &|name| {
        EventCategory::parse(name).is_some()
    })?
    .iter()
    .filter_map(|name| EventCategory::parse(name))
    .collect();
    // A kind is a name such as `client_steered`; one no event has matches nothing.
    let kinds = parse_all(query.kinds.as_deref(), "event kinds", &|name| {
        (1..=40).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    })?;
    let node = |id: &str| id.parse::<i64>().ok().filter(|id| *id > 0).map(NodeId::new);
    let nodes = parse_all(query.nodes.as_deref(), "node IDs", &|id| node(id).is_some())?
        .iter()
        .filter_map(|id| node(id))
        .collect();
    let clients = parse_all(query.clients.as_deref(), "clients' MAC addresses", &|mac| {
        mac.parse::<MacAddr>().is_ok()
    })?
    .iter()
    .filter_map(|mac| mac.parse().ok())
    .collect();
    let min = query
        .min
        .as_deref()
        .map(|name| {
            EventLevel::parse(name).ok_or_else(|| {
                ApiError::BadRequest(format!(
                    "{name:?} is not a level: use critical, warning, notice or info"
                ))
            })
        })
        .transpose()?;
    Ok(EventFilter {
        categories,
        kinds,
        min,
        nodes,
        clients,
        from: query.from.map(Timestamp::from_unix_ms),
        to: query.to.map(Timestamp::from_unix_ms),
    })
}

fn list_of(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `<time>:<id>`, as a page's `next` gives it.
fn cursor(text: &str) -> ApiResult<(Timestamp, EventId)> {
    text.split_once(':')
        .and_then(|(time, id)| Some((time.parse().ok()?, id.parse().ok()?)))
        .map(|(time, id)| (Timestamp::from_unix_ms(time), EventId::new(id)))
        .ok_or_else(|| ApiError::BadRequest(format!("{text:?} is not a page cursor")))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use uniwrt_core::{Clock as _, SystemClock};
    use uniwrt_model::events::{EventKind, EventWrite, NewEvent};
    use uniwrt_store::EventRepo as _;

    use crate::testing::{Demo, json};

    fn kinds(body: &serde_json::Value) -> Vec<&str> {
        body["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["what"]["kind"].as_str().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn events_are_found_a_page_at_a_time() {
        let demo = Demo::start_with(false).await;
        let now = SystemClock.now();
        let ago = |secs: u64| now.saturating_sub(std::time::Duration::from_secs(secs));
        let (gw, ap) = (demo.ids[0], demo.ids[1]);
        let phone = "f0:18:98:00:00:01".parse().unwrap();
        let events = vec![
            NewEvent::new(
                ago(300),
                EventKind::WanDown {
                    device: "eth1".into(),
                },
            )
            .node(gw),
            NewEvent::new(ago(200), EventKind::ClientLeft { connected_ms: None })
                .node(ap)
                .client(phone),
            NewEvent::new(ago(100), EventKind::NodeAdopted).node(ap),
        ];
        demo.store
            .record_events(events.into_iter().map(EventWrite::Insert).collect())
            .await
            .unwrap();
        let get = |uri: String| {
            let demo = &demo;
            async move { json(demo.send(demo.get(&uri)).await).await }
        };
        let body = get("/v1/events".into()).await;
        // The demo's own events (its start, its nodes) may come along: look at ours.
        let ours: Vec<_> = kinds(&body)
            .into_iter()
            .filter(|kind| ["wan_down", "client_left", "node_adopted"].contains(kind))
            .collect();
        assert_eq!(ours, ["node_adopted", "client_left", "wan_down"]);
        let body = get(format!("/v1/events?categories=wan,client&nodes={gw},{ap}")).await;
        assert_eq!(kinds(&body), ["client_left", "wan_down"]);
        let body = get(format!(
            "/v1/events?kinds=wan_down,client_left&nodes={gw},{ap}"
        ))
        .await;
        assert_eq!(kinds(&body), ["client_left", "wan_down"]);
        let body = get(format!("/v1/events?kinds=client_left&nodes={gw}")).await;
        assert_eq!(kinds(&body), Vec::<&str>::new());
        let body = get("/v1/events?min=critical&categories=wan".into()).await;
        assert_eq!(kinds(&body), ["wan_down"]);
        let body = get(format!("/v1/events?clients={phone}")).await;
        assert_eq!(kinds(&body), ["client_left"]);
        let body = get(format!("/v1/events?nodes={ap}&limit=1")).await;
        assert_eq!(kinds(&body), ["node_adopted"]);
        let next = body["next"].as_str().unwrap().to_owned();
        let body = get(format!("/v1/events?nodes={ap}&limit=1&before={next}")).await;
        assert_eq!(kinds(&body), ["client_left"]);
        for bad in [
            "/v1/events?categories=weather",
            "/v1/events?kinds=Client-Left",
            "/v1/events?min=loud",
            "/v1/events?nodes=x",
            "/v1/events?clients=nope",
            "/v1/events?limit=0",
            "/v1/events?before=x",
        ] {
            let res = demo.send(demo.get(bad)).await;
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
    }
}

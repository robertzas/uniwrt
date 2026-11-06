//! uniwrt's HTTP API (ARCHITECTURE.md, API): the `/api` routes, RFC 9457 problem+json errors,
//! the security headers, authentication with sessions, the nodes, and the WS hub.
//!
//! - **API:** [`Api::new`] from an [`ApiConfig`] (the store, the engine as an
//!   `Arc<dyn Fleet>`, the alert engine's handle, the notifications' handle with the vault, RF
//!   insight's handle, drift detection's handle, the vault for the site's secrets, the clock, the build identity, the HTTPS [`Certificate`], the password
//!   cost and the setup window); [`router`] for the server to nest under `/api`;
//!   [`Api::close_sockets`] when the server stops; [`security::headers`] for every response, UI
//!   included; [`problem`] for errors, [`no_route`] for unknown API paths. The listener sets
//!   each request's [`Client`].
//! - **Routes:** public: `GET v1/health`, `GET v1/tls`, `GET v1/tls/ca.crt`, `GET v1/auth`
//!   (setup state and the session's user), `POST v1/auth/setup`, `POST v1/auth/login`; with a
//!   session: `POST v1/auth/logout`, `GET v1/auth/me`, `GET v1/nodes` ([`NodeSummary`]s),
//!   `GET v1/nodes/{id}` ([`NodeDetail`]), `PUT v1/nodes/{id}/management` (admins: management
//!   on or off, a job), the apply (admins: `GET v1/apply/plan` ([`ApplyPlan`](uniwrt_model::apply::ApplyPlan)), `POST v1/apply`
//!   ([`ApplyBody`], a job), `GET v1/apply/applied`; see `apply`), the import (admins: `POST
//!   v1/import/preview` ([`ImportPreview`](uniwrt_model::import::ImportPreview)), `POST
//!   v1/import/accept` ([`ImportAccept`]); see `import`), drift (`GET v1/drift`
//!   ([`DriftState`](uniwrt_model::drift::DriftState)), admins: `POST
//!   v1/nodes/{id}/drift/{package}/acknowledge`, `PUT v1/nodes/{id}/drift/{package}/mute`
//!   ([`DriftMuteBody`]), and a drift's import (`{"drift": …}`, [`ImportRequest`]); see
//!   `drift`), the WAN wizard (`GET v1/isp/presets`, admins: `POST v1/wan/test`
//!   ([`WanTestBody`], a job); see `wan`), `POST v1/adopt/probe`, `POST v1/adopt`,
//!   `POST v1/adopt/batch` (one password for several nodes), `GET v1/discovered` (unadopted
//!   OpenWrt devices the adopted nodes see), `GET v1/jobs[/{id}]`, `GET v1/topology`, its
//!   replay (`GET v1/topology/timeline`, `GET v1/topology/at/{ms}`) and layout (`GET` and
//!   `DELETE v1/topology/layout`, `PUT` and `DELETE v1/topology/layout/{vertex}`), the clients
//!   (`GET v1/clients`, `GET` and `PATCH v1/clients/{mac}` ([`ClientDetail`], a rename by an
//!   admin), `GET v1/clients/{mac}/sessions?since=<ms>`), the metrics (`GET v1/metrics`, an
//!   entity's series downsampled to a width ([`MetricsResponse`]), and `GET v1/metrics/series`,
//!   what is kept ([`SeriesInfo`])), the logs (`GET v1/logs` pages ([`LogPage`]),
//!   `GET v1/logs/histogram`, `GET v1/logs/{id}/context`, `GET v1/logs/sources`,
//!   `GET v1/logs/patterns`, and saved searches: `GET` and `POST v1/logs/searches`,
//!   `DELETE v1/logs/searches/{id}`; see `logs`), the events (`GET v1/events` pages
//!   ([`EventPage`]); see `events`), the alerts (`GET v1/alerts` pages ([`AlertPage`]),
//!   `POST v1/alerts/{id}/acknowledge`, the rules under `v1/alert-rules` ([`RuleView`]) and the
//!   maintenance windows under `v1/maintenance`; see `alerts`), the notifiers (`GET` and
//!   `POST v1/notifiers` ([`NotifierList`], [`NotifierInput`]), `PUT` and
//!   `DELETE v1/notifiers/{id}`, `POST v1/notifiers/test` ([`NotifierTest`]) and
//!   `PUT v1/notifications/settings`; secrets go to the vault and come back only as which are
//!   set ([`SecretsView`]); see `notifiers`), RF insight (`GET v1/rf` ([`RfView`]), `POST` and
//!   `GET v1/rf/scans` ([`ScanRequest`], [`ScansQueued`]), `GET v1/rf/scans/{id}`,
//!   `PUT v1/rf/settings`; see `rf`), the site's desired state (`GET` and `PUT v1/site`
//!   ([`SiteView`], [`SiteSave`]), `POST v1/site/secrets/reveal` ([`RevealSecret`]), and its
//!   history: `GET v1/revisions` ([`RevisionPage`]), `GET v1/revisions/{id}`,
//!   `GET v1/revisions/{id}/diff` ([`RevisionDiff`]), `POST v1/revisions/{id}/restore`
//!   ([`RestoreBody`]); see `site`), and `v1/ws`
//!   (the hub: GET, or CONNECT over HTTP/2; topics `fleet`, `node:<id>`,
//!   `logs[?node=<id>|host=<name>&min=<severity>]`, `events[?node=<id>&min=<level>]`, `alerts`,
//!   `jobs`, `topology`, `clients`, `client:<mac>`, `rf`, see `ws`). Handlers that need a user
//!   take [`CurrentUser`].
//! - **Auth** (features/security.md; D-040): setup creates the admin only within 10 minutes of a
//!   start without users. Passwords are Argon2id (19 MiB, 2 passes) on the blocking pool, two at
//!   a time; unknown users cost the same as wrong passwords. Logins are throttled per client and
//!   per username. Sessions are 256-bit tokens in a `__Host-` cookie (`HttpOnly`, `Secure`,
//!   SameSite=Strict), stored as SHA-256, ending after 7 idle days or 30 in all. Setup, logins,
//!   failed logins and logouts go to the audit log.
//! - **Invariants:** state-changing requests need `X-Uniwrt-Csrf: 1` and, when they send one, our
//!   `Origin` ([`security`]). API responses are never cached. Errors never leak internals: a 500
//!   says to read the log, which has the cause. Passwords and tokens are zeroized and never logged.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model` (with `ts`), `uniwrt-store`, `uniwrt-engine`
//!   (`Fleet`, its events, `Alerts`, `Notify`), `uniwrt-events` (the built-in alert rules),
//!   `uniwrt-notify` (checking notifiers, their secrets), `uniwrt-node` (collect sets), `uniwrt-topo` and `uniwrt-rf` (with `ts`), axum (with `ws`), argon2, ring
//!   (tokens, salts, digests), ts-rs for the DTOs (`web/src/api/gen`).
//! - **Tests:** unit tests per module; `auth/tests.rs` drives the router with
//!   `tower::ServiceExt::oneshot` over a temporary store, on tokio's paused clock where time
//!   matters; `ws/subs_tests.rs` merges deltas over a scripted fleet; `nodes.rs` and
//!   `ws/tests.rs` run against the simulated fleet (`testing::Demo`), the hub over loopback
//!   HTTP/1.1 and HTTP/2 (`cargo nextest run -p uniwrt-api`).

mod adopt;
mod alerts;
mod apply;
pub mod auth;
mod client;
mod clients;
mod discovered;
mod drift;
mod events;
mod health;
mod import;
mod jobs;
mod logs;
mod management;
mod metrics;
mod nodes;
mod notifiers;
pub mod problem;
mod rf;
#[cfg(test)]
mod rf_tests;
pub mod security;
mod site;
mod state;
#[cfg(test)]
mod testing;
mod tls;
mod topology;
mod wan;
#[cfg(test)]
mod wan_tests;
mod ws;

use axum::{
    Router, middleware,
    routing::{delete, get, post, put},
};

pub use adopt::{
    AdoptBody, ProbeRequest, ProbeResult,
    batch::{BatchAdoptBody, BatchAdoptResult, BatchNode, Refused},
};
pub use alerts::{
    AlertPage,
    maintenance::WindowList,
    rules::{RuleList, RuleView},
};
pub use apply::ApplyBody;
pub use auth::{password::PasswordCost, session::CurrentUser, setup::SETUP_WINDOW};
pub use client::Client;
pub use clients::{ClientDetail, ClientPatch};
pub use drift::DriftMuteBody;
pub use events::EventPage;
pub use health::BuildInfo;
pub use import::{DriftTarget, ImportAccept, ImportRequest};
pub use logs::{
    LogContext, LogHistogram, LogPage, LogPatterns, LogSourceList, NewSearch, SavedSearches,
};
pub use management::ManagementBody;
pub use metrics::{MetricSeries, MetricsResponse, SeriesInfo};
pub use nodes::{NodeDetail, NodeSummary};
pub use notifiers::{
    HeaderInput, NotifierInput, NotifierList, NotifierTest, NotifierView, Placeholder,
    SecretsInput, SecretsView,
};
pub use problem::{ApiError, ApiResult, no_route};
pub use rf::{ScanRequest, ScansQueued};
pub use site::{
    FirstSite, RevealSecret, RevealedSecret, SecretState, SiteSave, SiteView,
    revisions::{RestoreBody, RevisionDiff, RevisionPage},
};
pub use state::{Api, ApiConfig};
pub use tls::{Certificate, TlsInfo};
pub use wan::WanTestBody;
pub use ws::{ClientMessage, FactsPatch, ServerMessage, TimedEvent, TimedRoam};

/// The API's routes, to nest under `/api`.
pub fn router(api: Api) -> Router {
    Router::new()
        .route("/v1/health", get(health::get))
        .route("/v1/tls", get(tls::info))
        .route("/v1/tls/ca.crt", get(tls::ca))
        .route("/v1/auth", get(auth::state))
        .route("/v1/auth/setup", post(auth::setup))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/logout", post(auth::logout))
        .route("/v1/auth/me", get(auth::me))
        .route("/v1/nodes", get(nodes::list))
        .route("/v1/nodes/{id}", get(nodes::get))
        .route("/v1/nodes/{id}/management", put(management::put))
        .route("/v1/apply", post(apply::start))
        .route("/v1/apply/plan", get(apply::plan))
        .route("/v1/apply/applied", get(apply::applied))
        .route("/v1/import/preview", post(import::preview))
        .route("/v1/drift", get(drift::get))
        .route(
            "/v1/nodes/{id}/drift/{package}/acknowledge",
            post(drift::acknowledge),
        )
        .route("/v1/nodes/{id}/drift/{package}/mute", put(drift::mute))
        .route("/v1/import/accept", post(import::accept))
        .route("/v1/adopt/probe", post(adopt::probe))
        .route("/v1/adopt", post(adopt::adopt))
        .route("/v1/adopt/batch", post(adopt::batch::adopt))
        .route("/v1/discovered", get(discovered::list))
        .route("/v1/jobs", get(jobs::list))
        .route("/v1/jobs/{id}", get(jobs::get))
        .route("/v1/clients", get(clients::list))
        .route("/v1/clients/{mac}", get(clients::get).patch(clients::patch))
        .route("/v1/clients/{mac}/sessions", get(clients::sessions))
        .route("/v1/events", get(events::list))
        .route("/v1/alerts", get(alerts::list))
        .route("/v1/alerts/{id}/acknowledge", post(alerts::acknowledge))
        .route(
            "/v1/alert-rules",
            get(alerts::rules::list).post(alerts::rules::create),
        )
        .route(
            "/v1/alert-rules/{id}",
            put(alerts::rules::update).delete(alerts::rules::delete),
        )
        .route("/v1/alert-rules/{id}/reset", post(alerts::rules::reset))
        .route(
            "/v1/maintenance",
            get(alerts::maintenance::list).post(alerts::maintenance::create),
        )
        .route("/v1/maintenance/{id}", delete(alerts::maintenance::delete))
        .route(
            "/v1/notifiers",
            get(notifiers::list).post(notifiers::create),
        )
        .route("/v1/notifiers/test", post(notifiers::test))
        .route(
            "/v1/notifiers/{id}",
            put(notifiers::update).delete(notifiers::delete),
        )
        .route("/v1/notifications/settings", put(notifiers::put_settings))
        .route("/v1/logs", get(logs::list))
        .route("/v1/logs/histogram", get(logs::histogram))
        .route("/v1/logs/sources", get(logs::sources))
        .route("/v1/logs/patterns", get(logs::patterns))
        .route(
            "/v1/logs/searches",
            get(logs::searches).post(logs::save_search),
        )
        .route("/v1/logs/searches/{id}", delete(logs::delete_search))
        .route("/v1/logs/{id}/context", get(logs::context))
        .route("/v1/metrics", get(metrics::get))
        .route("/v1/metrics/series", get(metrics::series))
        .route("/v1/rf", get(rf::get))
        .route("/v1/rf/scans", get(rf::scans).post(rf::scan))
        .route("/v1/rf/scans/{id}", get(rf::one))
        .route("/v1/rf/settings", put(rf::put_settings))
        .route("/v1/site", get(site::get).put(site::put))
        .route("/v1/isp/presets", get(wan::presets))
        .route("/v1/wan/test", post(wan::test))
        .route("/v1/site/secrets/reveal", post(site::reveal))
        .route("/v1/revisions", get(site::revisions::list))
        .route("/v1/revisions/{id}", get(site::revisions::get))
        .route("/v1/revisions/{id}/diff", get(site::revisions::changes))
        .route("/v1/revisions/{id}/restore", post(site::revisions::restore))
        .route("/v1/topology", get(topology::get))
        .route("/v1/topology/timeline", get(topology::history::timeline))
        .route("/v1/topology/at/{ms}", get(topology::history::at))
        .route(
            "/v1/topology/layout",
            get(topology::layout::list).delete(topology::layout::reset),
        )
        .route(
            "/v1/topology/layout/{vertex}",
            put(topology::layout::pin).delete(topology::layout::unpin),
        )
        // Over HTTP/2, browsers open WebSockets with an extended CONNECT (RFC 8441).
        .route("/v1/ws", get(ws::upgrade).connect(ws::upgrade))
        .fallback(problem::not_found)
        .method_not_allowed_fallback(problem::method_not_allowed)
        .layer(middleware::from_fn(security::csrf))
        .layer(middleware::map_response(security::no_store))
        .with_state(api)
}

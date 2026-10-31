//! uniwrt's HTTP API (ARCHITECTURE.md, API): the `/api` routes, RFC 9457 problem+json errors,
//! the security headers, authentication with sessions, the nodes, and the WS hub.
//!
//! - **API:** [`Api::new`] from an [`ApiConfig`] (the store, the engine as an
//!   `Arc<dyn Fleet>`, the clock, the build identity, the HTTPS [`Certificate`], the password
//!   cost and the setup window); [`router`] for the server to nest under `/api`;
//!   [`Api::close_sockets`] when the server stops; [`security::headers`] for every response, UI
//!   included; [`problem`] for errors, [`no_route`] for unknown API paths. The listener sets
//!   each request's [`Client`].
//! - **Routes:** public: `GET v1/health`, `GET v1/tls`, `GET v1/tls/ca.crt`, `GET v1/auth`
//!   (setup state and the session's user), `POST v1/auth/setup`, `POST v1/auth/login`; with a
//!   session: `POST v1/auth/logout`, `GET v1/auth/me`, `GET v1/nodes` ([`NodeSummary`]s),
//!   `GET v1/nodes/{id}` ([`NodeDetail`]), `POST v1/adopt/probe`, `POST v1/adopt`,
//!   `POST v1/adopt/batch` (one password for several nodes), `GET v1/discovered` (unadopted
//!   OpenWrt devices the adopted nodes see), `GET v1/jobs[/{id}]`, `GET v1/topology`, its
//!   replay (`GET v1/topology/timeline`, `GET v1/topology/at/{ms}`) and layout (`GET` and
//!   `DELETE v1/topology/layout`, `PUT` and `DELETE v1/topology/layout/{vertex}`), and `v1/ws`
//!   (the hub: GET, or CONNECT over HTTP/2; topics `fleet`, `node:<id>`,
//!   `logs?node=<id>&min=<severity>`, `jobs`, `topology`, see `ws`). Handlers that need a user
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
//!   (`Fleet`, its events), `uniwrt-node` (collect sets), `uniwrt-topo` (with `ts`), axum (with `ws`), argon2, ring
//!   (tokens, salts, digests), ts-rs for the DTOs (`web/src/api/gen`).
//! - **Tests:** unit tests per module; `auth/tests.rs` drives the router with
//!   `tower::ServiceExt::oneshot` over a temporary store, on tokio's paused clock where time
//!   matters; `ws/subs_tests.rs` merges deltas over a scripted fleet; `nodes.rs` and
//!   `ws/tests.rs` run against the simulated fleet (`testing::Demo`), the hub over loopback
//!   HTTP/1.1 and HTTP/2 (`cargo nextest run -p uniwrt-api`).

mod adopt;
pub mod auth;
mod client;
mod discovered;
mod health;
mod jobs;
mod nodes;
pub mod problem;
pub mod security;
mod state;
#[cfg(test)]
mod testing;
mod tls;
mod topology;
mod ws;

use axum::{
    Router, middleware,
    routing::{get, post, put},
};

pub use adopt::{
    AdoptBody, ProbeRequest, ProbeResult,
    batch::{BatchAdoptBody, BatchAdoptResult, BatchNode, Refused},
};
pub use auth::{password::PasswordCost, session::CurrentUser, setup::SETUP_WINDOW};
pub use client::Client;
pub use health::BuildInfo;
pub use nodes::{NodeDetail, NodeSummary};
pub use problem::{ApiError, ApiResult, no_route};
pub use state::{Api, ApiConfig};
pub use tls::{Certificate, TlsInfo};
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
        .route("/v1/adopt/probe", post(adopt::probe))
        .route("/v1/adopt", post(adopt::adopt))
        .route("/v1/adopt/batch", post(adopt::batch::adopt))
        .route("/v1/discovered", get(discovered::list))
        .route("/v1/jobs", get(jobs::list))
        .route("/v1/jobs/{id}", get(jobs::get))
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

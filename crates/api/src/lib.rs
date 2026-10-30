//! uniwrt's HTTP API (ARCHITECTURE.md, API): the `/api` routes, RFC 9457 problem+json errors,
//! the security headers, and authentication with sessions.
//!
//! - **API:** [`Api::new`] from an [`ApiConfig`] (the store, the clock, the build identity, the
//!   HTTPS [`Certificate`], the password cost and the setup window); [`router`] for the server to
//!   nest under `/api`; [`security::headers`] for every response, UI included; [`problem`] for
//!   errors, [`no_route`] for unknown API paths. The listener sets each request's [`Client`].
//! - **Routes:** public: `GET v1/health`, `GET v1/tls`, `GET v1/tls/ca.crt`, `GET v1/auth`
//!   (setup state and the session's user), `POST v1/auth/setup`, `POST v1/auth/login`; with a
//!   session: `POST v1/auth/logout`, `GET v1/auth/me`. Handlers that need a user take
//!   [`CurrentUser`].
//! - **Auth** (features/security.md; D-040): setup creates the admin only within 10 minutes of a
//!   start without users. Passwords are Argon2id (19 MiB, 2 passes) on the blocking pool, two at
//!   a time; unknown users cost the same as wrong passwords. Logins are throttled per client and
//!   per username. Sessions are 256-bit tokens in a `__Host-` cookie (`HttpOnly`, `Secure`,
//!   SameSite=Strict), stored as SHA-256, ending after 7 idle days or 30 in all. Setup, logins,
//!   failed logins and logouts go to the audit log.
//! - **Invariants:** state-changing requests need `X-Uniwrt-Csrf: 1` and, when they send one, our
//!   `Origin` ([`security`]). API responses are never cached. Errors never leak internals: a 500
//!   says to read the log, which has the cause. Passwords and tokens are zeroized and never logged.
//! - **Dependencies:** `uniwrt-core`, `uniwrt-model`, `uniwrt-store`, axum, argon2, ring (tokens,
//!   salts, digests), ts-rs for the DTOs (`web/src/api/gen`).
//! - **Tests:** unit tests per module; `auth/tests.rs` drives the router with
//!   `tower::ServiceExt::oneshot` over a temporary store, on tokio's paused clock where time
//!   matters (`cargo nextest run -p uniwrt-api`).

pub mod auth;
mod client;
mod health;
pub mod problem;
pub mod security;
mod state;
mod tls;

use axum::{
    Router, middleware,
    routing::{get, post},
};

pub use auth::{password::PasswordCost, session::CurrentUser, setup::SETUP_WINDOW};
pub use client::Client;
pub use health::BuildInfo;
pub use problem::{ApiError, ApiResult, no_route};
pub use state::{Api, ApiConfig};
pub use tls::{Certificate, TlsInfo};

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
        .fallback(problem::not_found)
        .method_not_allowed_fallback(problem::method_not_allowed)
        .layer(middleware::from_fn(security::csrf))
        .layer(middleware::map_response(security::no_store))
        .with_state(api)
}

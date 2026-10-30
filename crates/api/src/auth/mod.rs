//! Authentication (features/security.md): first-run setup within its window, login with
//! Argon2id passwords and throttling, sessions in a cookie, logout, and the audit of each.
//!
//! - `GET /api/v1/auth`: the setup state and the session's user, if any (public).
//! - `POST /api/v1/auth/setup`: creates the admin while the window is open, and logs them in.
//! - `POST /api/v1/auth/login`, `POST /api/v1/auth/logout`, `GET /api/v1/auth/me`.

pub(crate) mod limit;
pub(crate) mod password;
pub(crate) mod session;
pub(crate) mod setup;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use ts_rs::TS;
use uniwrt_model::user::{AuditAction, AuditEntry, Role, User};
use uniwrt_store::{AuditRepo as _, SessionRepo as _, UserRepo as _};
use zeroize::Zeroizing;

use crate::{Api, ApiError, ApiResult, Client};
use password::MIN_PASSWORD_CHARS;
use session::CurrentUser;
use setup::SetupState;

/// What the login page needs to know.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct AuthState {
    pub(crate) setup: SetupState,
    /// While setup is open, the seconds until it closes.
    pub(crate) setup_closes_in_s: Option<u64>,
    /// The session's user; `null` when not logged in.
    pub(crate) user: Option<UserInfo>,
    pub(crate) min_password_chars: usize,
}

/// A user, as the UI shows them.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct UserInfo {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) role: UserRole,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum UserRole {
    Admin,
    Viewer,
}

impl From<&User> for UserInfo {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.get(),
            username: user.username.clone(),
            role: match user.role {
                Role::Admin => UserRole::Admin,
                Role::Viewer => UserRole::Viewer,
            },
        }
    }
}

/// A username and password, for setup and login. Not `Debug`: it holds a password.
#[derive(Deserialize, TS)]
#[ts(export)]
pub(crate) struct Credentials {
    pub(crate) username: String,
    pub(crate) password: String,
}

impl Credentials {
    fn split(self) -> (String, Zeroizing<String>) {
        (self.username, Zeroizing::new(self.password))
    }
}

pub(crate) async fn state(
    State(api): State<Api>,
    headers: HeaderMap,
) -> ApiResult<Json<AuthState>> {
    let user = match session::presented(&headers) {
        Some(token) => session::current(&api, token).await?,
        None => None,
    };
    let (setup, left) = api.setup().state();
    Ok(Json(AuthState {
        setup,
        setup_closes_in_s: left.map(|left| left.as_secs() + u64::from(left.subsec_nanos() > 0)),
        user: user.map(|current| UserInfo::from(&current.user)),
        min_password_chars: MIN_PASSWORD_CHARS,
    }))
}

pub(crate) async fn setup(
    State(api): State<Api>,
    client: Client,
    headers: HeaderMap,
    body: Result<Json<Credentials>, JsonRejection>,
) -> ApiResult<Response> {
    match api.setup().state().0 {
        SetupState::Open => {}
        SetupState::Done => return Err(ApiError::Conflict("Setup is done; log in".to_owned())),
        SetupState::Closed => {
            return Err(ApiError::Forbidden(
                "Setup closed 10 minutes after the controller started; restart it to reopen setup"
                    .to_owned(),
            ));
        }
    }
    let (username, password) = body?.0.split();
    let username = password::username(&username)?;
    password::check_new(&password)?;
    let hash = api.passwords().hash(password).await?;
    let user = match api
        .store()
        .create_first_user(&username, Role::Admin, &hash)
        .await
    {
        Ok(user) => user,
        Err(uniwrt_store::Error::Conflict { .. }) => {
            return Err(ApiError::Conflict("Setup is done; log in".to_owned()));
        }
        Err(err) => return Err(err.into()),
    };
    api.setup().finish();
    audit(&api, AuditAction::Setup, Some(&user), None, &client).await?;
    info!(username = %user.username, ip = %client.ip, "first-run setup created the admin");
    logged_in(&api, &user, &client, &headers).await
}

pub(crate) async fn login(
    State(api): State<Api>,
    client: Client,
    headers: HeaderMap,
    body: Result<Json<Credentials>, JsonRejection>,
) -> ApiResult<Response> {
    let (username, password) = body?.0.split();
    let username = username.trim().to_ascii_lowercase();
    if let Some(wait) = api.limiter().wait(client.ip, &username) {
        return Err(ApiError::TooManyRequests(wait));
    }
    let credentials = if username.len() <= 64 {
        api.store().credentials(&username).await?
    } else {
        None
    };
    let (user, hash) = credentials.unzip();
    if !api.passwords().verify(password, hash).await? {
        api.limiter().failed(client.ip, &username);
        let name = username.chars().take(64).collect::<String>();
        warn!(username = %name, ip = %client.ip, "failed login");
        audit(
            &api,
            AuditAction::LoginFailed,
            user.as_ref(),
            Some(name),
            &client,
        )
        .await?;
        return Err(ApiError::Unauthorized(
            "Wrong username or password".to_owned(),
        ));
    }
    let Some(user) = user else {
        return Err(ApiError::Internal(
            "verified a password without a user".to_owned(),
        ));
    };
    api.limiter().succeeded(&username);
    audit(&api, AuditAction::Login, Some(&user), None, &client).await?;
    info!(username = %user.username, ip = %client.ip, "login");
    // Logins are rare: a good moment to drop the sessions that ended.
    let idle_since = api.now().saturating_sub(session::IDLE);
    api.store().delete_stale_sessions(idle_since).await?;
    logged_in(&api, &user, &client, &headers).await
}

pub(crate) async fn logout(
    State(api): State<Api>,
    client: Client,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if let Some(token) = session::presented(&headers) {
        if let Some(current) = session::current(&api, token).await? {
            audit(
                &api,
                AuditAction::Logout,
                Some(&current.user),
                None,
                &client,
            )
            .await?;
        }
        api.store().delete_session(token).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session::clear_cookie())],
    )
        .into_response())
}

pub(crate) async fn me(current: CurrentUser) -> Json<UserInfo> {
    Json(UserInfo::from(&current.user))
}

async fn logged_in(
    api: &Api,
    user: &User,
    client: &Client,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let token = session::start(api, user, client, headers).await?;
    Ok((
        [(header::SET_COOKIE, token.cookie())],
        Json(UserInfo::from(user)),
    )
        .into_response())
}

async fn audit(
    api: &Api,
    action: AuditAction,
    user: Option<&User>,
    username: Option<String>,
    client: &Client,
) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action,
            user_id: user.map(|user| user.id),
            username: username.or_else(|| user.map(|user| user.username.clone())),
            ip: Some(client.ip.to_string()),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;

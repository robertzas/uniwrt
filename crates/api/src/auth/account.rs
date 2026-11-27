//! Changing your own password (features/security.md, Users; D-153): `POST /api/v1/auth/password`
//! with the current password and the new one, for any role. The current password is verified and
//! throttled as a login's is; the user's other sessions end, the one asking stays.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use tracing::{info, warn};
use ts_rs::TS;
use uniwrt_model::user::{AuditAction, AuditEntry, User};
use uniwrt_store::{AuditRepo as _, UserChange, UserRepo as _};
use zeroize::Zeroizing;

use super::{password, session, session::CurrentUser};
use crate::{Api, ApiError, ApiResult, Client};

/// The current password and the new one. Not `Debug`: it holds passwords.
#[derive(Deserialize, TS)]
#[ts(export)]
pub(crate) struct PasswordChange {
    pub(crate) current: String,
    pub(crate) new: String,
}

/// What a wrong current password answers. Not 401, which would end the session.
pub(crate) const WRONG: &str = "The current password is wrong";

pub(crate) async fn change_password(
    State(api): State<Api>,
    current: CurrentUser,
    client: Client,
    headers: HeaderMap,
    body: Result<Json<PasswordChange>, JsonRejection>,
) -> ApiResult<StatusCode> {
    let PasswordChange { current: old, new } = body?.0;
    let (old, new) = (Zeroizing::new(old), Zeroizing::new(new));
    let user = current.user;
    if let Some(wait) = api.limiter().wait(client.ip, &user.username) {
        return Err(ApiError::TooManyRequests(wait));
    }
    password::check_new(&new)?;
    let hash = api
        .store()
        .credentials(&user.username)
        .await?
        .map(|(_, hash)| hash);
    if !api.passwords().verify(old, hash).await? {
        api.limiter().failed(client.ip, &user.username);
        warn!(username = %user.username, ip = %client.ip, "a password change with a wrong current password");
        audit(
            &api,
            &user,
            &client,
            "refused: the current password was wrong",
        )
        .await?;
        return Err(ApiError::BadRequest(WRONG.to_owned()));
    }
    api.limiter().succeeded(&user.username);
    let change = UserChange {
        role: None,
        password_hash: Some(api.passwords().hash(new).await?),
    };
    api.store()
        .update_user(user.id, change, session::presented(&headers))
        .await?;
    audit(&api, &user, &client, "changed; other sessions ended").await?;
    info!(username = %user.username, ip = %client.ip, "password changed");
    Ok(StatusCode::NO_CONTENT)
}

async fn audit(api: &Api, user: &User, client: &Client, detail: &str) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Password,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail.to_owned()),
        })
        .await?;
    Ok(())
}

//! Users (features/security.md, Users; D-153), for admins: the list with each user's last
//! activity, adding one with a role and a first password, changing a role or setting a new
//! password, and deleting one. A change ends that user's sessions; the last admin can't be
//! demoted or deleted, and an admin can neither delete themselves nor set their own password
//! here (`POST /api/v1/auth/password` asks for the current one).
//!
//! - `GET /api/v1/users`, `POST /api/v1/users`
//! - `PATCH /api/v1/users/{id}`, `DELETE /api/v1/users/{id}`

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use tracing::info;
use ts_rs::TS;
use uniwrt_core::{Timestamp, UserId};
use uniwrt_model::user::{AuditAction, AuditEntry, Role, User};
use uniwrt_store::{AuditRepo as _, UserChange, UserRepo as _};
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    auth::{UserInfo, UserRole, password, session},
};

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{get, patch};
    axum::Router::new()
        .route("/v1/users", get(list).post(create))
        .route("/v1/users/{id}", patch(update).delete(remove))
}

/// A user as the Users tab lists them.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub(crate) struct Account {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) role: UserRole,
    pub(crate) created_at: Timestamp,
    pub(crate) updated_at: Timestamp,
    /// The last request made with one of their sessions, while one is kept.
    pub(crate) last_seen_at: Option<Timestamp>,
}

/// A user to add. Not `Debug`: it holds a password.
#[derive(Deserialize, TS)]
#[ts(export)]
pub(crate) struct NewUser {
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) role: UserRole,
}

/// What to change; a field left out stays. Not `Debug`: it may hold a password.
#[derive(Deserialize, TS)]
#[ts(export)]
pub(crate) struct UserPatch {
    #[serde(default)]
    #[ts(optional)]
    pub(crate) role: Option<UserRole>,
    /// A new password, which the admin hands over.
    #[serde(default)]
    #[ts(optional)]
    pub(crate) password: Option<String>,
}

pub(crate) async fn list(
    State(api): State<Api>,
    current: CurrentUser,
) -> ApiResult<Json<Vec<Account>>> {
    admin(&current.user)?;
    let users = api.store().users().await?;
    Ok(Json(
        users
            .into_iter()
            .map(|activity| Account {
                id: activity.user.id.get(),
                username: activity.user.username,
                role: activity.user.role.into(),
                created_at: activity.user.created_at,
                updated_at: activity.user.updated_at,
                last_seen_at: activity.last_seen_at,
            })
            .collect(),
    ))
}

pub(crate) async fn create(
    State(api): State<Api>,
    current: CurrentUser,
    client: Client,
    body: Result<Json<NewUser>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<UserInfo>)> {
    admin(&current.user)?;
    let NewUser {
        username,
        password,
        role,
    } = body?.0;
    let password = Zeroizing::new(password);
    let username = password::username(&username)?;
    password::check_new(&password)?;
    let hash = api.passwords().hash(password).await?;
    let user = match api.store().create_user(&username, role.into(), &hash).await {
        Ok(user) => user,
        Err(uniwrt_store::Error::Conflict { .. }) => {
            return Err(ApiError::Conflict(format!(
                "A user named {username} already exists"
            )));
        }
        Err(err) => return Err(err.into()),
    };
    let detail = format!("added {} ({})", user.username, role_name(user.role));
    audit(&api, &current.user, &client, detail).await?;
    Ok((StatusCode::CREATED, Json(UserInfo::from(&user))))
}

pub(crate) async fn update(
    State(api): State<Api>,
    current: CurrentUser,
    client: Client,
    headers: HeaderMap,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<UserPatch>, JsonRejection>,
) -> ApiResult<Json<UserInfo>> {
    admin(&current.user)?;
    let id = user_id(id)?;
    let UserPatch { role, password } = body?.0;
    let password = password.map(Zeroizing::new);
    let hash = match password {
        Some(_) if id == current.user.id => {
            return Err(ApiError::BadRequest(
                "Change your own password from your account menu, with your current one".to_owned(),
            ));
        }
        Some(password) => {
            password::check_new(&password)?;
            Some(api.passwords().hash(password).await?)
        }
        None => None,
    };
    let before = api.store().user(id).await?.ok_or_else(|| not_found(id))?;
    let set_password = hash.is_some();
    let change = UserChange {
        role: role.map(Role::from),
        password_hash: hash,
    };
    // The session asking stays, even when it is the user's own.
    let user = api
        .store()
        .update_user(id, change, session::presented(&headers))
        .await
        .map_err(|err| refused(err, id))?;
    let mut changes = Vec::new();
    if user.role != before.role {
        changes.push(format!(
            "{} → {}",
            role_name(before.role),
            role_name(user.role)
        ));
    }
    if set_password {
        changes.push("password set".to_owned());
    }
    if !changes.is_empty() {
        let detail = format!("{}: {}; sessions ended", user.username, changes.join(", "));
        info!(admin = %current.user.username, %detail, "user changed");
        audit(&api, &current.user, &client, detail).await?;
    }
    Ok(Json(UserInfo::from(&user)))
}

pub(crate) async fn remove(
    State(api): State<Api>,
    current: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<StatusCode> {
    admin(&current.user)?;
    let id = user_id(id)?;
    if id == current.user.id {
        return Err(ApiError::Conflict(
            "You can't delete your own account; another admin can".to_owned(),
        ));
    }
    let user = api
        .store()
        .delete_user(id)
        .await
        .map_err(|err| refused(err, id))?;
    info!(admin = %current.user.username, username = %user.username, "user deleted");
    audit(
        &api,
        &current.user,
        &client,
        format!("deleted {}", user.username),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn admin(user: &User) -> ApiResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(crate::auth::roles::REFUSAL.to_owned()))
    }
}

fn user_id(id: Result<Path<i64>, PathRejection>) -> ApiResult<UserId> {
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A user ID is a number".to_owned()))?;
    Ok(UserId::new(id))
}

fn not_found(id: UserId) -> ApiError {
    ApiError::NotFound(format!("No user has ID {}", id.get()))
}

/// The store's refusals as answers.
fn refused(err: uniwrt_store::Error, id: UserId) -> ApiError {
    match err {
        uniwrt_store::Error::NotFound { .. } => not_found(id),
        err => err.into(),
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Viewer => "viewer",
    }
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Users,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "users_tests.rs"]
mod tests;

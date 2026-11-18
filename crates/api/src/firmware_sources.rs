//! Where upgrade images come from (M7.1a2b, D-122), for admins; shown by `GET v1/firmware`:
//!
//! - `PUT /api/v1/firmware/sources` ([`SourcesBody`]; [`SiteSources`]): the site's GitHub repo
//!   (`owner/name` or its link; none: OpenWrt's), whether prereleases are offered, and the
//!   token (kept, set or cleared; sealed in the vault, never shown again). A repo GitHub says
//!   doesn't exist (or the token can't read) is refused; GitHub unreachable doesn't block the
//!   save. Audited (`firmware`).
//! - `PUT /api/v1/firmware/nodes/{id}/source` ([`NodeSource`]): a node's own source (the site's,
//!   ASU, official images or another repo), checked the same way. Audited.
//!
//! Kept as the `firmware_sources` setting ([`FirmwareSources`]); the token as the vault's
//! [`TOKEN`].

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_core::NodeId;
use uniwrt_firmware::{Error, parse_repo, valid_token};
use uniwrt_model::{
    firmware::{FirmwareSources, NodeSource, SiteSources},
    user::{AuditAction, AuditEntry, User},
};
use uniwrt_store::{AuditRepo as _, NodeRepo as _, SettingsRepo as _, VaultRepo as _};
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    firmware::{admin, firmware_error},
};

/// The vault's name for the GitHub token.
pub(crate) const TOKEN: &str = "firmware/github-token";

pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::put;
    axum::Router::new()
        .route("/v1/firmware/sources", put(put_sources))
        .route("/v1/firmware/nodes/{id}/source", put(put_node_source))
}

/// The site's sources as an admin sets them.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SourcesBody {
    /// `owner/name` or the repo's link; `None` or empty: OpenWrt's releases.
    pub repo: Option<String>,
    pub prereleases: bool,
    pub token: TokenChange,
}

/// What to do with the GitHub token.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
#[ts(export)]
pub enum TokenChange {
    Keep,
    /// A fine-grained token with read access to the repos' contents.
    Set {
        token: String,
    },
    Clear,
}

/// The sources as kept; the defaults when none are.
pub(crate) async fn load(api: &Api) -> ApiResult<FirmwareSources> {
    Ok(match api.store().setting(FirmwareSources::KEY).await? {
        Some(value) => serde_json::from_value(value).unwrap_or_default(),
        None => FirmwareSources::default(),
    })
}

/// The GitHub token, when one is kept (and the vault is open).
pub(crate) async fn token(api: &Api) -> ApiResult<Option<Zeroizing<String>>> {
    let Ok(vault) = api.vault() else {
        return Ok(None);
    };
    Ok(vault
        .secret(TOKEN)
        .await?
        .and_then(|secret| String::from_utf8(secret.to_vec()).ok().map(Zeroizing::new)))
}

/// The site's sources as shown.
pub(crate) fn view(sources: &FirmwareSources, token: bool) -> SiteSources {
    SiteSources {
        repo: sources.repo.clone(),
        prereleases: sources.prereleases,
        token,
    }
}

pub(crate) async fn put_sources(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<SourcesBody>, JsonRejection>,
) -> ApiResult<Json<SiteSources>> {
    let user = user.user;
    admin(&user)?;
    let Json(body) = body?;
    let repo = repo(body.repo.as_deref())?;
    let kept = token(&api).await?;
    let token = match &body.token {
        TokenChange::Keep => kept.clone(),
        TokenChange::Set { token } => {
            let token = token.trim();
            if !valid_token(token) {
                return Err(ApiError::BadRequest(
                    "A GitHub token is printable text without spaces".to_owned(),
                ));
            }
            Some(Zeroizing::new(token.to_owned()))
        }
        TokenChange::Clear => None,
    };
    if let Some(repo) = &repo {
        check(&api, repo, token.as_deref().map(String::as_str)).await?;
    }
    match &body.token {
        TokenChange::Keep => {}
        TokenChange::Set { .. } => {
            let secret = token
                .as_ref()
                .map_or_default(|token| token.as_bytes().to_vec());
            api.vault()?
                .put_secret(TOKEN, Zeroizing::new(secret))
                .await?;
        }
        TokenChange::Clear => {
            api.vault()?.delete_secret(TOKEN).await?;
        }
    }
    let mut sources = load(&api).await?;
    sources.repo = repo;
    sources.prereleases = body.prereleases;
    save(&api, &sources).await?;
    let token_word = match body.token {
        TokenChange::Keep => "kept",
        TokenChange::Set { .. } => "set",
        TokenChange::Clear => "removed",
    };
    let detail = format!(
        "set the firmware sources: {}, prereleases {}, the GitHub token {token_word}",
        sources.repo.as_deref().map_or_else(
            || "OpenWrt's releases".to_owned(),
            |repo| format!("GitHub {repo}")
        ),
        if sources.prereleases {
            "offered"
        } else {
            "not offered"
        },
    );
    audit(&api, &user, &client, detail).await?;
    Ok(Json(view(&sources, token.is_some())))
}

pub(crate) async fn put_node_source(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<NodeSource>, JsonRejection>,
) -> ApiResult<Json<NodeSource>> {
    let user = user.user;
    admin(&user)?;
    let Path(id) = id.map_err(|_| ApiError::BadRequest("A node ID is a number".to_owned()))?;
    let id = NodeId::new(id);
    let Json(source) = body?;
    let node = api
        .store()
        .node(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("There is no node {id}")))?;
    let source = match source {
        NodeSource::Github { repo: typed } => {
            let repo = repo(Some(&typed))?
                .ok_or_else(|| ApiError::BadRequest("Name the node's GitHub repo".to_owned()))?;
            let token = token(&api).await?;
            check(&api, &repo, token.as_deref().map(String::as_str)).await?;
            NodeSource::Github { repo }
        }
        other => other,
    };
    let mut sources = load(&api).await?;
    if source == NodeSource::Site {
        sources.nodes.remove(&id);
    } else {
        sources.nodes.insert(id, source.clone());
    }
    save(&api, &sources).await?;
    let detail = format!(
        "set the firmware source of {} (node {id}) to {}",
        node.name,
        match &source {
            NodeSource::Site => "the site's".to_owned(),
            NodeSource::Asu => "OpenWrt built by ASU".to_owned(),
            NodeSource::Official => "OpenWrt's official images".to_owned(),
            NodeSource::Github { repo } => format!("GitHub {repo}"),
        }
    );
    audit(&api, &user, &client, detail).await?;
    Ok(Json(source))
}

/// A repo as typed, normalized; `None` when empty.
fn repo(typed: Option<&str>) -> ApiResult<Option<String>> {
    match typed.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => parse_repo(text).map(Some).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "{text} isn't a GitHub repo: name it as owner/name, or paste its link"
            ))
        }),
    }
}

/// Refuses a repo GitHub says isn't there (or the token can't read, or a token it refused);
/// GitHub unreachable or its rate limit spent doesn't block a save.
async fn check(api: &Api, repo: &str, token: Option<&str>) -> ApiResult<()> {
    let github = api.github()?;
    github.forget();
    match github.releases(repo, token).await {
        Ok(_) => Ok(()),
        Err(Error::NotFound(why) | Error::Unauthorized(why)) => Err(ApiError::BadRequest(why)),
        Err(err) => {
            tracing::warn!(%repo, err = %firmware_error(err), "cannot check the GitHub repo");
            Ok(())
        }
    }
}

async fn save(api: &Api, sources: &FirmwareSources) -> ApiResult<()> {
    let value = serde_json::to_value(sources).map_err(|err| ApiError::Internal(err.to_string()))?;
    api.store().put_setting(FirmwareSources::KEY, value).await?;
    Ok(())
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Firmware,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}
